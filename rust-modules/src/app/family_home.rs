//! Private declarative launcher configuration and the owned background playback loop.
use plx_media::player::background::Source;
use plx_screens::family_home::Application;
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {version:u32,pub background:Option<Source>,apps:Vec<ApplicationConfig>,#[serde(default)] controls:Option<super::family_input::Config>,#[serde(default)] rendering:Option<super::family_scene::Config>}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationConfig{id:String,title:String,#[serde(default)] icon:Option<String>,#[serde(default)] scene:Option<String>,#[serde(skip)] pixels:Option<plx_ui::owned_image::Pixels>}
pub(crate) struct Runtime {config:Config,retry_at:u32,status_at:u32,snapshot_at:u32,snapshot:Option<Snapshot>,worker:Option<std::sync::mpsc::Receiver<Option<Snapshot>>>,input:Option<super::family_input::Bridge>,scene_click:bool,scene:Option<super::family_scene::Controller>,managed:Option<Source>,foreground:bool,wake:bool}
impl Config {
    pub fn load()->Result<Self,String>{
        use std::io::Read;
        let path=format!("/media/internal/.{}-home.json",plx_base::paths::app_id());
        let file=std::fs::File::open(path).map_err(|_|"family configuration unavailable")?;
        let mut bytes=Vec::new();file.take(32769).read_to_end(&mut bytes).map_err(|_|"family configuration unreadable")?;
        if bytes.len()>32768{return Err("family configuration oversized".into());}
        let mut config:Self=serde_json::from_slice(&bytes).map_err(|_|"family configuration invalid")?;
        if config.rendering.as_ref().is_some_and(|rendering|!rendering.valid())||config.version!=1||!(1..=5).contains(&config.apps.len())||config.apps.iter().map(|app|&app.id).collect::<std::collections::BTreeSet<_>>().len()!=config.apps.len()||config.apps.iter().any(|app|app.id.is_empty()||app.id.len()>128
            ||!app.id.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'.'||c==b'-'||c==b'_')||app.title.is_empty()||app.title.len()>80
            ||app.title.chars().any(char::is_control)||app.scene.as_ref().is_some_and(|scene|!super::family_scene::scene_id(scene)||config.rendering.is_none()))||config.background.as_ref().is_some_and(|source|!source.valid())||config.controls.as_ref().is_some_and(|controls|!controls.valid(config.background.as_ref().map(|source|source.url.as_str()))){
            return Err("family configuration rejected".into());
        }
        for app in &mut config.apps { app.pixels=app.icon.as_deref().and_then(load_icon); }
        Ok(config)
    }
    pub fn applications(&self)->Vec<Application>{self.apps.iter().map(|app|Application{id:app.id.clone(),title:app.title.clone(),icon:app.pixels.clone()}).collect()}
    pub fn empty()->Self{Self{version:1,background:None,apps:Vec::new(),controls:None,rendering:None}}
}
impl Runtime {
    pub fn application(&self,index:u8)->Option<&str>{self.config.apps.get(index as usize).map(|app|app.id.as_str())}
    pub fn new(mut config:Config)->Self{let scene=config.rendering.take().map(super::family_scene::Controller::new);let input=config.controls.as_ref().and_then(super::family_input::Bridge::start);Self{config,retry_at:0,status_at:0,snapshot_at:0,snapshot:None,worker:None,input,scene_click:false,scene,managed:None,foreground:false,wake:false}}
    pub fn launch_scene(&mut self,index:u8)->bool{
        let Some(scene)=self.config.apps.get(index as usize).and_then(|app|app.scene.as_deref())else{return false;};
        if let Some(controller)=&mut self.scene{controller.select(scene);true}else{false}
    }
    pub fn home_scene(&mut self)->bool{let changed=self.scene.as_mut().is_some_and(super::family_scene::Controller::home);if changed{self.wake=true;}changed}
    pub fn relaunch(&mut self){self.home_scene();self.wake=true;}
    pub fn take_wake(&mut self)->bool{std::mem::take(&mut self.wake)}
    pub fn release_controls(&mut self){self.scene_click=false;if let Some(input)=&self.input{input.release();}}
    pub fn controls_enabled(&self)->bool{self.input.is_some()}
    pub fn control_pointer(&mut self,x:f32,y:f32,on_control:bool){
        if let Some(input)=&self.input{input.pointer(if on_control{None}else{plx_screens::family_home::scene_point(self.config.apps.len(),x,y)});}
    }
    pub fn control_click(&mut self,down:bool)->bool{
        let Some(input)=&self.input else{return false;};
        if down{self.scene_click=false;if input.engaged(){self.scene_click=true;input.activate();return true;}return false;}
        if !down&&self.scene_click{self.scene_click=false;return true;}false
    }
    pub fn control_zoom(&self,delta:f32)->bool{self.input.as_ref().is_some_and(|input|{if input.engaged(){input.zoom(delta);true}else{false}})}
    pub fn control_key(&self,key:&'static str,down:bool)->bool{self.input.as_ref().is_some_and(|input|{let engaged=input.engaged();input.key(key,down);engaged})}
    pub fn status(&mut self,now:u32)->Option<plx_screens::family_home::Status>{
        if now.wrapping_sub(self.status_at)>=u32::MAX/2{return None;}
        self.status_at=now.wrapping_add(1000);
        if let Some(worker)=&self.worker {
            match worker.try_recv(){Ok(snapshot)=>{self.snapshot=snapshot;self.worker=None;},Err(std::sync::mpsc::TryRecvError::Disconnected)=>{self.worker=None;},Err(std::sync::mpsc::TryRecvError::Empty)=>{}}
        }
        if self.worker.is_none()&&now.wrapping_sub(self.snapshot_at)<u32::MAX/2{
            self.snapshot_at=now.wrapping_add(25000);
            let (send,recv)=std::sync::mpsc::sync_channel(1);
            if plx_base::task::spawn_small("family status",move||{let _=send.send(read_snapshot());}){self.worker=Some(recv);}
        }
        let unix=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        let network=self.snapshot.as_ref().filter(|snapshot|snapshot.fresh(unix)).map(|snapshot|snapshot.label.clone()).unwrap_or_else(||"—".into());
        let reading=plx_platform::wall_clock::read()?;
        let locale=plx_platform::i18n::current();
        let clock=if locale.clock()==plx_platform::i18n::Clock::H12{
            format!("{}:{:02} {}",if reading.hour%12==0{12}else{reading.hour%12},reading.minute,if reading.hour<12{"AM"}else{"PM"})
        }else{format!("{:02}:{:02}",reading.hour,reading.minute)};
        Some(plx_screens::family_home::Status{clock,date:locale.date(reading.year,reading.month,reading.day).unwrap_or_default(),network})
    }
    pub fn tick(&mut self,ps:&mut plx_media::route::PlaybackSession,pa:&mut plx_media::player::adapter::PlayerAdapter,now:u32,foreground:bool){
        if foreground&&!self.foreground{self.wake=true;}self.foreground=foreground;
        if let Some(change)=self.scene.as_mut().and_then(|scene|scene.poll(foreground)){
            let switching=self.managed.is_some()||matches!(&change,super::family_scene::Change::Ready(_));
            self.release_controls();
            if switching&&plx_media::route::is_background(ps)&&pa.is_live(){plx_media::player::stop_bufferfeed(ps,pa);}
            match change {super::family_scene::Change::Ready(grant)=>{self.managed=Some(grant.source());self.input=grant.input().as_ref().and_then(super::family_input::Bridge::start);},super::family_scene::Change::Passive=>{self.managed=None;self.input=self.config.controls.as_ref().and_then(super::family_input::Bridge::start);}}
            if switching{self.retry_at=now;}
        }
        if !foreground{return;}
        if self.managed.is_some()&&pa.is_live()&&plx_media::player::ended(){
            if let Some(scene)=&mut self.scene{scene.failed();}
            self.release_controls();self.managed=None;self.input=None;
            if pa.is_live(){plx_media::player::stop_bufferfeed(ps,pa);}
            self.retry_at=now;
        }
        if self.input.is_some()&&(!pa.is_live()||!plx_media::route::is_background(ps)||plx_media::player::ended()){self.release_controls();}
        let Some(source)=self.managed.as_ref().or(self.config.background.as_ref()) else{return;};
        if plx_media::player::ended()&&plx_media::route::is_background(ps)&&source.looping{
            plx_media::player::stop_bufferfeed(ps,pa);
        }
        if !pa.is_live()&&now.wrapping_sub(self.retry_at)<u32::MAX/2{
            let started=plx_media::player::background::start(ps,pa,source);
            if !started&&self.managed.is_some(){
                if let Some(scene)=&mut self.scene{scene.failed();}
                self.release_controls();self.managed=None;self.input=None;
            }
            self.retry_at=now.wrapping_add(5000);
        }
    }
}

// Administrative local PNGs only; bound file bytes and IHDR dimensions before decoding.
fn load_icon(path:&str)->Option<plx_ui::owned_image::Pixels>{
    use std::io::Read;
    if !path.starts_with("/media/internal/"){return None;}
    let mut bytes=Vec::new();std::fs::File::open(path).ok()?.take(1048577).read_to_end(&mut bytes).ok()?;
    if bytes.len()>1048576||bytes.len()<24||&bytes[..8]!=b"\x89PNG\r\n\x1a\n"{return None;}
    let w=u32::from_be_bytes(bytes[16..20].try_into().ok()?);let h=u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if w==0||h==0||w>512||h>512{return None;}
    let (w,h,rgba)=plx_gfx::img::img_decode_owned(&bytes)?;
    plx_ui::owned_image::Pixels::new(w.try_into().ok()?,h.try_into().ok()?,rgba)
}

struct Snapshot {observed:u64,label:String}
impl Snapshot {fn fresh(&self,now:u64)->bool{self.observed<=now.saturating_add(60)&&now.saturating_sub(self.observed)<=120}}
fn read_snapshot()->Option<Snapshot>{
    use std::io::Read;
    let mut bytes=Vec::new();std::fs::File::open("/media/internal/pinpin-family/home-status.json").ok()?.take(4097).read_to_end(&mut bytes).ok()?;
    if bytes.len()>4096{return None;}
    snapshot(&bytes)
}
fn snapshot(bytes:&[u8])->Option<Snapshot>{
    let value:serde_json::Value=serde_json::from_slice(bytes).ok()?;
    if value["schema"].as_u64()!=Some(1){return None;}
    let observed=value["observedAtUnix"].as_u64()?;
    let network=&value["network"];
    if network["status"].as_str()!=Some("ready")||network["connected"].as_bool()!=Some(true){return None;}
    let label=match network["transport"].as_str()?{
        "wifi"=>match network["signalBars"].as_u64(){Some(bars@0..=4)=>format!("Wi-Fi · {bars}/4"),_=>"Wi-Fi".into()},
        "ethernet"=>"Ethernet".into(),_=>return None,
    };
    Some(Snapshot{observed,label})
}
#[cfg(test)]
mod snapshot_tests{
    use super::*;
    #[test]
    fn real_provider_contract_is_fresh_only_with_a_valid_observation_and_connection(){
        let bytes=br#"{"schema":1,"observedAtUnix":1000,"network":{"status":"ready","transport":"wifi","connected":true,"internet":true,"signalBars":3},"weather":{"status":"unconfigured","city":null,"temperatureC":null}}"#;
        let result=snapshot(bytes).unwrap();assert_eq!(result.label,"Wi-Fi · 3/4");assert!(result.fresh(1120));assert!(!result.fresh(1121));assert!(!result.fresh(939));
        for corrupt in [String::from_utf8_lossy(bytes).replace("\"observedAtUnix\":1000,",""),String::from_utf8_lossy(bytes).replace("\"connected\":true","\"connected\":false"),String::from_utf8_lossy(bytes).replace("\"ready\"","\"stale\"")]{assert!(snapshot(corrupt.as_bytes()).is_none());}
    }
}

#[cfg(test)]
mod control_tests{
    use super::*;
    #[test]
    fn a_new_dock_press_cannot_lose_its_release_after_a_missing_scene_button_up(){
        let mut config=Config::empty();config.apps.push(ApplicationConfig{id:"example".into(),title:"Example".into(),icon:None,scene:None,pixels:None});
        let mut runtime=Runtime::new(config);runtime.input=Some(super::super::family_input::Bridge::for_test());
        runtime.control_pointer(100.0,100.0,false);assert!(runtime.control_click(true));
        // The old scene's button-up was lost. A new dock gesture owns both of its edges.
        runtime.control_pointer(960.0,900.0,true);assert!(!runtime.control_click(true));assert!(!runtime.control_click(false));
    }
}
