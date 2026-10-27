//! Foreground ownership of a shared Mini scene. HTTP stays on bounded workers.
use plx_media::player::background::Source;
use std::sync::mpsc::{Receiver,TryRecvError};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {#[serde(rename="queryURL")] query_url:String,#[serde(default="default_scene")] scene:String}
fn default_scene()->String{"forest".into()}
pub(super) fn scene_id(id:&str)->bool{matches!(id,"forest"|"atlas"|"alambique")}
fn origin(url:&str)->Option<plx_net::net::origin::Origin>{
    if url.len()>4096||url.chars().any(char::is_control)||url.contains('#'){return None;}
    let port=if url.starts_with("https://"){443}else if url.starts_with("http://"){80}else{return None;};
    plx_net::net::origin::Origin::parse_connection(url,port)
}
impl Config {pub fn valid(&self)->bool{scene_id(&self.scene)&&origin(&self.query_url).is_some()}}

#[derive(Clone,serde::Deserialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Grant {
    session_id:String,scene:String,state:String,#[serde(rename="videoURL")] video_url:String,vcodec:String,fps:f64,width:u16,height:u16,
    video_only:bool,controls:Controls,
}
#[derive(Clone,serde::Deserialize)]
#[serde(rename_all="camelCase")]
struct Controls {#[serde(rename="inputURL")] input_url:String,#[serde(rename="heartbeatURL")] heartbeat_url:String,#[serde(rename="stopURL")] stop_url:String}
impl Grant {
    fn valid(&self,query:&str)->bool{
        !self.session_id.is_empty()&&self.session_id.len()<=128
            &&self.session_id.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'-'|b'_'))
            &&scene_id(&self.scene)&&self.state=="ready"&&self.video_only&&self.source().valid()
            &&[&self.video_url,&self.controls.input_url,&self.controls.heartbeat_url,&self.controls.stop_url]
                .into_iter().all(|url|origin(url).is_some()&&origin(url)==origin(query))
    }
    pub fn source(&self)->Source{Source{url:self.video_url.clone(),local_cache:None,video_codec:self.vcodec.clone(),fps:self.fps,width:self.width,height:self.height,looping:false}}
    pub fn input(&self)->Option<super::family_input::Config>{serde_json::from_value(serde_json::json!({"inputURL":self.controls.input_url})).ok()}
}
pub(crate) enum Change {Ready(Grant),Passive}
enum Reply {Start(Option<Grant>),Heartbeat(bool),Stopped}
pub(crate) struct Controller {
    config:Config,desired:Option<String>,active:Option<Grant>,pending:Option<Receiver<Reply>>,
    stops:Vec<Grant>,foreground:bool,next:u32,
}
impl Controller {
    pub fn new(config:Config)->Self{Self{desired:Some(config.scene.clone()),config,active:None,pending:None,stops:Vec::new(),foreground:false,next:super::clock::now()}}
    pub fn immersive(&self)->bool{self.active.as_ref().is_some_and(|grant|grant.scene!=self.config.scene)}
    pub fn select(&mut self,scene:&str){if scene_id(scene){self.desired=Some(scene.into());self.next=super::clock::now();}}
    pub fn home(&mut self)->bool{
        if self.desired.as_deref().is_some_and(|scene|scene!=self.config.scene){let scene=self.config.scene.clone();self.select(&scene);true}else{false}
    }
    fn retire(&mut self){if let Some(grant)=self.active.take(){self.stops.push(grant);}}
    fn accept(&mut self,reply:Reply,foreground:bool)->Option<Change>{
        match reply {
            Reply::Start(Some(grant)) if foreground&&self.desired.as_deref()==Some(grant.scene.as_str())=>{
                self.retire();self.active=Some(grant.clone());self.next=super::clock::now().wrapping_add(10000);Some(Change::Ready(grant))
            },
            Reply::Start(Some(grant))=>{self.stops.push(grant);None},
            Reply::Start(None)=>{
                self.retire();self.next=super::clock::now().wrapping_add(5000);
                Some(Change::Passive)
            },
            Reply::Heartbeat(false)=>{
                self.failed();
                Some(Change::Passive)
            },
            Reply::Heartbeat(true)=>{
                self.next=super::clock::now().wrapping_add(if self.active.as_ref().is_some_and(|grant|self.desired.as_deref()==Some(grant.scene.as_str())){10000}else{0});None
            },
            Reply::Stopped=>None,
        }
    }
    pub fn poll(&mut self,foreground:bool)->Option<Change>{
        let mut change=None;
        if !foreground&&self.foreground{self.desired=None;self.retire();change=Some(Change::Passive);}
        if foreground&&!self.foreground{self.desired=Some(self.config.scene.clone());self.next=super::clock::now();}
        self.foreground=foreground;
        let reply=self.pending.as_ref().and_then(|recv|match recv.try_recv(){Ok(reply)=>Some(reply),Err(TryRecvError::Disconnected)=>Some(Reply::Heartbeat(false)),Err(TryRecvError::Empty)=>None});
        if let Some(reply)=reply {
            self.pending=None;
            change=self.accept(reply,foreground).or(change);
        }
        if self.pending.is_none(){
            if let Some(grant)=self.stops.pop(){self.pending=worker(move||{let _=session_request(&grant,"render.scene.stop");Reply::Stopped});}
            else if foreground&&super::clock::now().wrapping_sub(self.next)<u32::MAX/2 {
                if let Some(scene)=self.desired.clone(){
                    if self.active.as_ref().is_some_and(|grant|grant.scene==scene){
                        let grant=self.active.as_ref().unwrap().clone();self.pending=worker(move||Reply::Heartbeat(session_request(&grant,"render.scene.heartbeat")));
                    }else{let query=self.config.query_url.clone();self.pending=worker(move||Reply::Start(start(&query,&scene)));}
                }
            }
        }
        change
    }
    pub fn failed(&mut self){
        // A failed displayed scene yields to Home once. Retrying it without another
        // selection repeatedly replaces the healthy local background with black.
        // A newer user selection must survive a failure from the previous scene.
        if self.active.as_ref().is_some_and(|grant|self.desired.as_deref()==Some(grant.scene.as_str())&&grant.scene!=self.config.scene){self.desired=Some(self.config.scene.clone());}
        self.retire();self.next=super::clock::now().wrapping_add(5000);
    }
}
impl Drop for Controller {fn drop(&mut self){if let Some(grant)=self.active.take(){let _=worker(move||{let _=session_request(&grant,"render.scene.stop");Reply::Stopped});}}}
fn worker(job:impl FnOnce()->Reply+Send+'static)->Option<Receiver<Reply>>{
    let (send,recv)=std::sync::mpsc::sync_channel(1);
    plx_base::task::spawn_small("shared scene",move||{let _=send.send(job());}).then_some(recv)
}
fn request(url:&str,body:serde_json::Value,millis:u16)->Option<serde_json::Value>{
    let body=serde_json::to_vec(&body).ok()?;
    let timeout=plx_net::net::Timeouts{connect_s:2,total_s:20,total_ms:millis.into(),low_speed_bps:0,low_speed_s:0};
    let response=plx_net::net::request(url,&["Content-Type: application/json".into()],"POST",Some(&body),timeout,false,Some(16384),None)?;
    if !response.ok(){return None;}
    let value:serde_json::Value=serde_json::from_slice(&response.body).ok()?;
    (value["ok"].as_bool()==Some(true)).then_some(value)
}
fn start(query:&str,scene:&str)->Option<Grant>{
    let value=request(query,serde_json::json!({"kind":"render.scene.start","params":{"scene":scene,"viewer":"family-home"}}),20000)?;
    let grant:Grant=serde_json::from_value(value["value"].clone()).ok()?;
    (grant.scene==scene&&grant.valid(query)).then_some(grant)
}
fn session_request(grant:&Grant,kind:&str)->bool{
    let url=if kind=="render.scene.stop"{&grant.controls.stop_url}else{&grant.controls.heartbeat_url};
    request(url,serde_json::json!({"kind":kind,"params":{"sessionId":grant.session_id}}),1000).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grant(scene:&str)->Grant{Grant{session_id:"test-session".into(),scene:scene.into(),state:"ready".into(),video_url:"http://192.0.2.1:4188/video.ts".into(),vcodec:"h264".into(),fps:30.0,width:1920,height:1080,video_only:true,controls:Controls{input_url:"http://192.0.2.1:4188/input".into(),heartbeat_url:"http://192.0.2.1:4188/heartbeat".into(),stop_url:"http://192.0.2.1:4188/stop".into()}}}
    fn controller()->Controller{Controller::new(Config{query_url:"http://192.0.2.1:4188/query".into(),scene:"forest".into()})}
    #[test]
    fn registry_and_grants_reject_unrequested_scene_or_cross_origin_capabilities(){
        assert!(scene_id("atlas"));assert!(!scene_id("../../shell"));
        let value=serde_json::json!({"sessionId":"session-1","scene":"atlas","state":"ready","videoURL":"http://192.0.2.1:4188/video.ts?cap=example","vcodec":"h264","fps":30,"width":1920,"height":1080,"videoOnly":true,"controls":{"inputURL":"http://192.0.2.1:4188/input","heartbeatURL":"http://192.0.2.1:4188/heartbeat","stopURL":"http://192.0.2.1:4188/stop"}});
        let mut grant:Grant=serde_json::from_value(value).unwrap();assert!(grant.valid("http://192.0.2.1:4188/query"));
        grant.controls.stop_url="http://192.0.2.2:4188/stop".into();assert!(!grant.valid("http://192.0.2.1:4188/query"));
        grant.controls.stop_url="http://192.0.2.1:4188/stop".into();grant.fps=60.0;assert!(grant.valid("http://192.0.2.1:4188/query"));
        grant.fps=61.0;assert!(!grant.valid("http://192.0.2.1:4188/query"));
        grant.fps=120.0;assert!(!grant.valid("http://192.0.2.1:4188/query"));
        grant.fps=60.0;grant.video_only=false;assert!(!grant.valid("http://192.0.2.1:4188/query"));
    }
    #[test]
    fn a_late_start_after_background_is_retired_without_becoming_visible(){
        let mut controller=controller();controller.desired=None;
        assert!(controller.accept(Reply::Start(Some(grant("atlas"))),false).is_none());
        assert!(controller.active.is_none());assert!(controller.desired.is_none());assert_eq!(controller.stops.len(),1);
        controller.desired=Some("alambique".into());
        assert!(controller.accept(Reply::Start(Some(grant("atlas"))),true).is_none());
        assert!(controller.active.is_none());assert_eq!(controller.stops.len(),2);
    }
    #[test]
    fn retry_deadline_uses_replay_time_and_wraps(){
        super::super::clock::set_replay(u32::MAX-2000);
        let mut controller=controller();controller.failed();assert_eq!(controller.next,2999);
        super::super::clock::set_replay(3000);controller.select("atlas");assert_eq!(controller.next,3000);
        super::super::clock::set_replay(0);
    }
    #[test]
    fn immersive_follows_accepted_active_scene_not_pending_choice(){
        let mut c=controller();c.select("atlas");assert!(!c.immersive());
        assert!(matches!(c.accept(Reply::Start(Some(grant("atlas"))),true),Some(Change::Ready(_))));assert!(c.immersive());
        c.home();assert!(c.immersive(),"pending default is not yet displayed");
        let _=c.accept(Reply::Start(Some(grant("forest"))),true);assert!(!c.immersive());
        c.active=Some(grant("atlas"));c.failed();assert!(!c.immersive());
    }
    #[test]
    fn scene_change_during_heartbeat_survives_old_failure_and_can_return_home(){
        let mut controller=controller();controller.active=Some(grant("forest"));controller.select("atlas");
        controller.accept(Reply::Heartbeat(true),true);assert!(controller.next<=super::super::clock::now());
        assert!(matches!(controller.accept(Reply::Heartbeat(false),true),Some(Change::Passive)));
        assert!(controller.active.is_none());assert_eq!(controller.desired.as_deref(),Some("atlas"));assert_eq!(controller.stops.len(),1);
        assert!(controller.home());assert!(!controller.home());
    }
    #[test]
    fn failed_selected_scene_returns_to_default_instead_of_repeating_black_loads(){
        let mut controller=controller();controller.select("atlas");controller.active=Some(grant("atlas"));
        controller.failed();
        assert_eq!(controller.desired.as_deref(),Some("forest"));
        assert!(controller.active.is_none());
        assert!(!controller.home(),"failed scene already yielded to the launcher");
    }
    #[test]
    fn failed_displayed_scene_does_not_erase_a_newer_user_selection(){
        let mut controller=controller();controller.active=Some(grant("atlas"));controller.select("alambique");
        controller.failed();assert_eq!(controller.desired.as_deref(),Some("alambique"));
    }
    #[test]
    fn lost_active_scene_heartbeat_also_yields_to_home_once(){
        let mut controller=controller();controller.select("atlas");controller.active=Some(grant("atlas"));
        assert!(matches!(controller.accept(Reply::Heartbeat(false),true),Some(Change::Passive)));
        assert_eq!(controller.desired.as_deref(),Some("forest"));assert_eq!(controller.stops.len(),1);
    }
}
