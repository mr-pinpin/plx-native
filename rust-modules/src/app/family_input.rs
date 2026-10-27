//! One optional, bounded return channel for native scene input. No scene owns network code.
use std::sync::{Arc,Condvar,Mutex};
use std::time::Duration;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {#[serde(rename="inputURL")] input_url:String}
impl Config {
    pub fn valid(&self,video_url:Option<&str>)->bool{
        self.input_url.len()<=4096&&!self.input_url.chars().any(char::is_control)
            &&!self.input_url.contains('#')&&endpoint_origin(&self.input_url).is_some()
            &&video_url.and_then(endpoint_origin)==endpoint_origin(&self.input_url)
    }
}
fn endpoint_origin(url:&str)->Option<plx_net::net::origin::Origin>{
    let default_port=if url.starts_with("https://"){443}else if url.starts_with("http://"){80}else{return None;};
    // The generic URL uses standard HTTP ports, rather than the PMS parser's legacy 32400 fallback.
    plx_net::net::origin::Origin::parse_connection(url,default_port)
}
#[derive(Clone,PartialEq,serde::Serialize)]
struct Packet {
    kind:&'static str,pointer:[f32;2],keys:Vec<&'static str>,activate:bool,zoom:f32,engaged:bool,
}
impl Packet {
    fn neutral()->Self{Self{kind:"input",pointer:[0.5,0.5],keys:Vec::new(),activate:false,zoom:0.0,engaged:false}}
}
struct Slot {position:bool,packet:Packet,updated:u32,generation:u64,stop:bool}
impl Slot {
    fn new()->Self{Self{position:false,packet:Packet::neutral(),updated:super::clock::now(),generation:0,stop:false}}
    fn touch(&mut self){self.updated=super::clock::now();self.generation=self.generation.wrapping_add(1);}
    fn take(&mut self,now:u32)->(Packet,u64){
        if !self.position&&now.wrapping_sub(self.updated)>750{self.packet=Packet::neutral();}
        let packet=self.packet.clone();self.packet.activate=false;self.packet.zoom=0.0;(packet,self.generation)
    }
}
pub(crate) struct Bridge {shared:Arc<(Mutex<Slot>,Condvar)>}
impl Bridge {
    pub fn start(config:&Config)->Option<Self>{
        let url=config.input_url.clone();
        Self::with_sender(move|packet|post_packet(&url,packet))
    }

    pub fn start_position(config:&Config)->Option<Self>{let url=config.input_url.clone();Self::with_sender_policy(true,move|packet|post_packet(&url,packet))}

    #[cfg(test)]
    pub(super) fn for_test()->Self{Self::with_sender(|_|true).unwrap()}
    fn with_sender(send:impl Fn(&Packet)->bool+Send+'static)->Option<Self>{
        Self::with_sender_policy(false,send)
    }
    fn with_sender_policy(position:bool,send:impl Fn(&Packet)->bool+Send+'static)->Option<Self>{
        let mut initial=Slot::new();initial.position=position;
        let shared=Arc::new((Mutex::new(initial),Condvar::new()));let worker=Arc::clone(&shared);
        plx_base::task::spawn_off_frame("native scene input",move|off|{
            let mut last:Option<Packet>=None;let mut next=plx_base::task::worker_now(off);let mut releasing=false;
            // Only the worker samples real time; frame-owned freshness uses replay ticks.
            let mut observed_generation=0;let mut observed_at=next;
            loop {
                let (packet,generation,stop)={
                    let (lock,wake)=&*worker;let mut slot=lock.lock().unwrap_or_else(|e|e.into_inner());
                    while !slot.stop&&plx_base::task::worker_now(off)<next {
                        let wait=next.saturating_duration_since(plx_base::task::worker_now(off));
                        slot=wake.wait_timeout(slot,wait).unwrap_or_else(|e|e.into_inner()).0;
                    }
                    if slot.stop {(Packet::neutral(),slot.generation,true)}else if releasing{(Packet::neutral(),slot.generation,false)}else{if slot.generation!=observed_generation{observed_generation=slot.generation;observed_at=plx_base::task::worker_now(off);}
                        if !slot.position&&plx_base::task::worker_now(off).saturating_duration_since(observed_at)>Duration::from_millis(750){slot.packet=Packet::neutral();}
                        let updated=slot.updated;let (packet,generation)=slot.take(updated);(packet,generation,false)}
                };
                if stop {let _=send(&packet);break;}
                let changed=last.as_ref()!=Some(&packet);
                let ok=if changed||packet.engaged||releasing {send(&packet)}else{true};
                last=Some(packet);releasing=!ok;next=plx_base::task::worker_now(off)+Duration::from_millis(if ok{50}else{250});
                if !ok {
                    let mut slot=worker.0.lock().unwrap_or_else(|e|e.into_inner());
                    // Managed positions remain desired intent; the acknowledged neutral barrier
                    // still precedes their retry. Explicit release updates the slot itself.
                    if slot.generation==generation&&!slot.position {slot.packet=Packet::neutral();}
                    // The peer's 750ms lease also releases controls if this best-effort neutral cannot arrive.
                    last=None;
                }
            }
        }).then_some(Self{shared})
    }
    fn update(&self,change:impl FnOnce(&mut Packet)){
        let mut slot=self.shared.0.lock().unwrap_or_else(|e|e.into_inner());change(&mut slot.packet);slot.touch();self.shared.1.notify_one();
    }
    pub fn pointer(&self,point:Option<[f32;2]>){self.update(|packet|match point{
        Some(point)=>{packet.pointer=point;packet.engaged=true;},None=>*packet=Packet::neutral(),
    });}
    pub fn engaged(&self)->bool{let slot=self.shared.0.lock().unwrap_or_else(|e|e.into_inner());slot.packet.engaged&&(slot.position||super::clock::now().wrapping_sub(slot.updated)<=750)}
    pub fn activate(&self){self.update(|packet|{if packet.engaged{packet.activate=true;}});}
    pub fn zoom(&self,delta:f32){self.update(|packet|{if packet.engaged&&delta.is_finite(){packet.zoom=(packet.zoom+delta).clamp(-32.0,32.0);}});}
    pub fn key(&self,key:&'static str,down:bool){if !matches!(key,"KeyW"|"KeyA"|"KeyS"|"KeyD"|"KeyE"|"Space"|"ShiftLeft"|"ShiftRight"){return;}self.update(|packet|{
        if down&&packet.engaged&&!packet.keys.contains(&key){packet.keys.push(key);}
        if !down{packet.keys.retain(|held|*held!=key);}
    });}
    pub fn release(&self){self.pointer(None);}
}
impl Drop for Bridge {fn drop(&mut self){let mut slot=self.shared.0.lock().unwrap_or_else(|e|e.into_inner());slot.stop=true;self.shared.1.notify_one();}}

fn post_packet(url:&str,packet:&Packet)->bool{
    let Ok(body)=serde_json::to_vec(packet)else{return false;};
    let deadlines=plx_net::net::Timeouts{connect_s:1,total_s:1,total_ms:250,low_speed_bps:0,low_speed_s:0};
    plx_net::net::request(url,&["Content-Type: application/json".into()],"POST",Some(&body),deadlines,false,Some(4096),None)
        .is_some_and(|response|response.ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_now()->std::time::Instant{plx_base::task::worker_now(&plx_base::task::OffFrame::for_test())}
    #[test]
    fn latest_state_retains_one_shot_edges_and_stale_input_releases_ambient_mode(){
        let mut slot=Slot::new();slot.packet.engaged=true;slot.packet.activate=true;slot.packet.zoom=2.0;
        for x in 0..100 {slot.packet.pointer=[x as f32/100.0,0.25];slot.touch();}
        let (first,_)=slot.take(slot.updated);assert_eq!(first.pointer,[0.99,0.25]);assert!(first.activate);assert_eq!(first.zoom,2.0);
        let (next,_)=slot.take(slot.updated);assert!(!next.activate);assert_eq!(next.zoom,0.0);assert!(next.engaged);
        slot.packet.keys.push("KeyW");let (expired,_)=slot.take(slot.updated.wrapping_add(751));assert!(expired==Packet::neutral());
    }
    #[test]
    fn managed_position_survives_stillness_until_explicit_release_but_legacy_expires(){
        let mut slot=Slot::new();slot.position=true;slot.packet.engaged=true;slot.packet.pointer=[0.8,0.2];
        let (packet,_)=slot.take(slot.updated.wrapping_add(60000));assert!(packet.engaged);assert_eq!(packet.pointer,[0.8,0.2]);
        slot.packet=Packet::neutral();assert!(!slot.take(slot.updated.wrapping_add(60001)).0.engaged);
        let mut legacy=Slot::new();legacy.packet.engaged=true;assert!(!legacy.take(legacy.updated.wrapping_add(751)).0.engaged);
    }
    #[test]
    fn replay_tick_expiry_is_wrap_safe(){
        let mut slot=Slot::new();slot.updated=u32::MAX-100;slot.packet.engaged=true;
        assert!(slot.take(649).0.engaged);assert!(!slot.take(650).0.engaged);
    }
    #[test]
    fn private_control_origin_must_match_explicit_background_stream(){
        let control=Config{input_url:"http://192.0.2.1:4188/__render/native/input?cap=example".into()};
        assert!(control.valid(Some("http://192.0.2.1:4188/__render/native/video.ts?cap=example")));
        assert!(!control.valid(Some("http://192.0.2.1:32400/library/parts/1/file.mp4")));
        assert!(!control.valid(None));
        let standard=Config{input_url:"http://192.0.2.1/input".into()};
        assert!(standard.valid(Some("http://192.0.2.1:80/video")));
        assert!(!standard.valid(Some("http://192.0.2.1:32400/video")));
        assert!(!Config{input_url:"192.0.2.1/input".into()}.valid(Some("http://192.0.2.1/video")));
    }
    #[test]
    fn worker_coalesces_while_a_request_is_in_flight_and_stops_with_neutral(){
        let (sent,seen)=std::sync::mpsc::channel();let (release,wait)=std::sync::mpsc::channel();
        let mut first=true;
        // The sender owns its request state behind a mutex because the production callback is Fn.
        let sender=Mutex::new(move|packet:&Packet|{sent.send(packet.clone()).unwrap();if first{first=false;wait.recv_timeout(Duration::from_secs(2)).unwrap();}true});
        let bridge=Bridge::with_sender(move|packet|sender.lock().unwrap()(packet)).unwrap();
        let initial=seen.recv_timeout(Duration::from_secs(2)).unwrap();assert!(initial==Packet::neutral());
        for index in 0..100 {bridge.pointer(Some([index as f32/100.0,0.25]));}
        bridge.activate();release.send(()).unwrap();
        let latest=seen.recv_timeout(Duration::from_secs(2)).unwrap();assert_eq!(latest.pointer,[0.99,0.25]);assert!(latest.activate);
        drop(bridge);
        let deadline=test_now()+Duration::from_secs(2);
        loop {let packet=seen.recv_timeout(deadline.saturating_duration_since(test_now())).unwrap();if packet==Packet::neutral(){break;}}
    }
    #[test]
    fn production_http_sender_posts_the_wire_packet_and_refuses_redirects_and_oversized_replies(){
        use std::io::{Read,Write};
        let _serial=plx_base::testlock::serial();
        assert!(plx_net::net::global_init());
        plx_net::net::set_user_agent("PinPinInputTest/1.0".into()).unwrap();
        for (status,size,accepted) in [(200,2,true),(302,2,false),(200,5000,false)] {
            let listener=std::net::TcpListener::bind(("127.0.0.1",0)).unwrap();
            let address=listener.local_addr().unwrap();listener.set_nonblocking(true).unwrap();
            let server=std::thread::spawn(move||{
                let deadline=test_now()+Duration::from_secs(2);
                let mut socket=loop{match listener.accept(){Ok((socket,_))=>break socket,Err(error)if error.kind()==std::io::ErrorKind::WouldBlock=>{
                    assert!(test_now()<deadline,"sender never reached loopback fixture");std::thread::sleep(Duration::from_millis(5));},Err(error)=>panic!("{error}")}};
                socket.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                let mut bytes=Vec::new();let mut chunk=[0u8;2048];
                let end=loop{let n=socket.read(&mut chunk).unwrap();assert!(n>0);bytes.extend_from_slice(&chunk[..n]);if let Some(at)=bytes.windows(4).position(|part|part==b"\r\n\r\n"){break at+4;}assert!(bytes.len()<16384);};
                let header=String::from_utf8_lossy(&bytes[..end]);
                let length=header.lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|value|value.trim().parse::<usize>().ok())).unwrap();
                while bytes.len()<end+length{let n=socket.read(&mut chunk).unwrap();assert!(n>0);bytes.extend_from_slice(&chunk[..n]);}
                assert!(String::from_utf8_lossy(&bytes[..end]).starts_with("POST /input?cap=synthetic HTTP/1.1"));
                let body:serde_json::Value=serde_json::from_slice(&bytes[end..end+length]).unwrap();assert_eq!(body["kind"],"input");assert_eq!(body["engaged"],false);assert!(body["keys"].as_array().unwrap().is_empty());
                let header=format!("HTTP/1.1 {status} Test\r\nContent-Length: {size}\r\nLocation: http://127.0.0.1:9/not-followed\r\nConnection: close\r\n\r\n");
                let _=socket.write_all(header.as_bytes());let _=socket.write_all(&vec![b' ';size]);
            });
            assert_eq!(post_packet(&format!("http://{address}/input?cap=synthetic"),&Packet::neutral()),accepted);
            server.join().unwrap();
        }
    }

    #[test]
    fn a_failed_request_releases_the_peer_before_newer_steering_can_resume(){
        let (sent,seen)=std::sync::mpsc::channel();let (resume,wait)=std::sync::mpsc::channel();
        let mut fail_once=true;
        let sender=Mutex::new(move|packet:&Packet|{
            sent.send(packet.clone()).unwrap();
            if packet.engaged&&fail_once{fail_once=false;wait.recv_timeout(Duration::from_secs(2)).unwrap();false}else{true}
        });
        let bridge=Bridge::with_sender(move|packet|sender.lock().unwrap()(packet)).unwrap();
        assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap()==Packet::neutral());
        bridge.pointer(Some([0.1,0.2]));assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap().engaged);
        bridge.pointer(Some([0.9,0.2]));resume.send(()).unwrap();
        assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap()==Packet::neutral());
        assert_eq!(seen.recv_timeout(Duration::from_secs(2)).unwrap().pointer,[0.9,0.2]);
        drop(bridge);
        let deadline=test_now()+Duration::from_secs(2);
        loop{if seen.recv_timeout(deadline.saturating_duration_since(test_now())).unwrap()==Packet::neutral(){break;}}
    }

    #[test]
    fn managed_position_recovers_lost_ack_without_motion_or_replaying_edges(){
        let (sent,seen)=std::sync::mpsc::channel();
        let mut failed=false;
        let sender=Mutex::new(move|packet:&Packet|{sent.send(packet.clone()).unwrap();if packet.engaged&&!failed{failed=true;false}else{true}});
        let bridge=Bridge::with_sender_policy(true,move|packet|sender.lock().unwrap()(packet)).unwrap();
        assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap()==Packet::neutral());
        bridge.update(|packet|{packet.pointer=[0.8,0.4];packet.engaged=true;packet.activate=true;packet.zoom=2.0;});
        let lost=seen.recv_timeout(Duration::from_secs(2)).unwrap();assert!(lost.engaged&&lost.activate);assert_eq!(lost.zoom,2.0);
        assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap()==Packet::neutral(),"failed steering must acknowledge neutral before resuming");
        let recovered=seen.recv_timeout(Duration::from_secs(1)).expect("stationary managed position must resume after neutral acknowledgement");
        assert_eq!(recovered.pointer,[0.8,0.4]);assert!(recovered.engaged);assert!(!recovered.activate);assert_eq!(recovered.zoom,0.0);
        drop(bridge);
        let deadline=test_now()+Duration::from_secs(2);
        loop{if seen.recv_timeout(deadline.saturating_duration_since(test_now())).unwrap()==Packet::neutral(){break;}}
    }

    #[test]
    fn explicit_release_during_lost_ack_never_resurrects_managed_position(){
        let (sent,seen)=std::sync::mpsc::channel();let (resume,wait)=std::sync::mpsc::channel();let mut failed=false;
        let sender=Mutex::new(move|packet:&Packet|{sent.send(packet.clone()).unwrap();if packet.engaged&&!failed{failed=true;wait.recv_timeout(Duration::from_secs(2)).unwrap();false}else{true}});
        let bridge=Bridge::with_sender_policy(true,move|packet|sender.lock().unwrap()(packet)).unwrap();
        assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap()==Packet::neutral());
        bridge.pointer(Some([0.8,0.4]));assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap().engaged);
        bridge.release();resume.send(()).unwrap();assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap()==Packet::neutral());
        assert!(seen.recv_timeout(Duration::from_millis(150)).is_err(),"explicit release must not re-arm old steering");
        drop(bridge);assert!(seen.recv_timeout(Duration::from_secs(2)).unwrap()==Packet::neutral());
    }

}
