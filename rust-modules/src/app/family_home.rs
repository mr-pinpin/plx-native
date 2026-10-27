//! Private declarative launcher configuration and the owned background playback loop.
use plx_media::player::background::Source;
use plx_screens::family_home::Application;
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {version:u32,pub background:Option<Source>,apps:Vec<ApplicationConfig>}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationConfig{id:String,title:String,#[serde(default)] icon:Option<String>,#[serde(skip)] pixels:Option<plx_ui::owned_image::Pixels>}
pub(crate) struct Runtime {config:Config,retry_at:u32,status_at:u32}
impl Config {
    pub fn load()->Result<Self,String>{
        use std::io::Read;
        let path=format!("/media/internal/.{}-home.json",plx_base::paths::app_id());
        let file=std::fs::File::open(path).map_err(|_|"family configuration unavailable")?;
        let mut bytes=Vec::new();file.take(32769).read_to_end(&mut bytes).map_err(|_|"family configuration unreadable")?;
        if bytes.len()>32768{return Err("family configuration oversized".into());}
        let mut config:Self=serde_json::from_slice(&bytes).map_err(|_|"family configuration invalid")?;
        if config.version!=1||!(1..=5).contains(&config.apps.len())||config.apps.iter().map(|app|&app.id).collect::<std::collections::BTreeSet<_>>().len()!=config.apps.len()||config.apps.iter().any(|app|app.id.is_empty()||app.id.len()>128
            ||!app.id.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'.'||c==b'-'||c==b'_')||app.title.is_empty()||app.title.len()>80
            ||app.title.chars().any(char::is_control))||config.background.as_ref().is_some_and(|source|!source.valid()){
            return Err("family configuration rejected".into());
        }
        for app in &mut config.apps { app.pixels=app.icon.as_deref().and_then(load_icon); }
        Ok(config)
    }
    pub fn applications(&self)->Vec<Application>{self.apps.iter().map(|app|Application{id:app.id.clone(),title:app.title.clone(),icon:app.pixels.clone()}).collect()}
    pub fn empty()->Self{Self{version:1,background:None,apps:Vec::new()}}
}
impl Runtime {
    pub fn application(&self,index:u8)->Option<&str>{self.config.apps.get(index as usize).map(|app|app.id.as_str())}
    pub fn new(config:Config)->Self{Self{config,retry_at:0,status_at:0}}
    pub fn status(&mut self,now:u32)->Option<plx_screens::family_home::Status>{
        if now.wrapping_sub(self.status_at)>=u32::MAX/2{return None;}
        self.status_at=now.wrapping_add(1000);
        let reading=plx_platform::wall_clock::read()?;
        let locale=plx_platform::i18n::current();
        let clock=if locale.clock()==plx_platform::i18n::Clock::H12{
            format!("{}:{:02} {}",if reading.hour%12==0{12}else{reading.hour%12},reading.minute,if reading.hour<12{"AM"}else{"PM"})
        }else{format!("{:02}:{:02}",reading.hour,reading.minute)};
        Some(plx_screens::family_home::Status{clock,date:locale.date(reading.year,reading.month,reading.day).unwrap_or_default()})
    }
    pub fn tick(&mut self,ps:&mut plx_media::route::PlaybackSession,pa:&mut plx_media::player::adapter::PlayerAdapter,now:u32){
        let Some(source)=&self.config.background else{return;};
        if plx_media::player::ended()&&plx_media::route::is_background(ps)&&source.looping{
            plx_media::player::stop_bufferfeed(ps,pa);
        }
        if !pa.is_live()&&now.wrapping_sub(self.retry_at)<u32::MAX/2{
            let _=plx_media::player::background::start(ps,pa,source);
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
