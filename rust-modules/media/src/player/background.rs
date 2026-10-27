//! Typed, video-only background playback through the production engine. No Plex identity,
//! queue or timeline is created; expected VOD EOS loops belong to this owner, not trailer budgets.
use super::adapter::PlayerAdapter;
use crate::route::PlaybackSession;

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    #[serde(default,rename="localCache")]
    pub local_cache: Option<crate::local_cache::Spec>,
    #[serde(rename="videoCodec")]
    pub video_codec: String,
    pub fps: f64,
    pub width: u16,
    pub height: u16,
    #[serde(default="loop_default",rename="loop")]
    pub looping: bool,
}
fn loop_default()->bool{true}
impl Source {
    pub fn valid(&self)->bool {
        (self.url.starts_with("http://")||self.url.starts_with("https://"))
            && self.local_cache.as_ref().is_none_or(|cache|cache.valid())
            && self.url.len()<=4096 && !self.url.chars().any(char::is_control)
            && matches!(self.video_codec.as_str(),"h264"|"hevc"|"h265")
            && self.fps.is_finite() && self.fps>0.0 && self.fps<=60.0
            && self.width>0 && self.height>0 && self.width<=4096 && self.height<=2176
            && (self.video_codec!="h264"||self.fps<=30.0)
    }
}
/// Called only with no engine installed. The complete declaration is published before any Load.
pub fn start(ps:&mut PlaybackSession,pa:&mut PlayerAdapter,source:&Source)->bool{
    if !source.valid()||pa.is_live(){return false;}
    *ps=PlaybackSession::default();
    if !crate::route::set_stream_declaration(ps,&source.video_codec,"",source.fps,plx_data::metadata::Dovi::NONE,false){return false;}
    crate::route::set_stream_source_raster(ps,source.width,source.height);
    crate::route::mark_background(ps);
    crate::route::set_background_cache(ps,source.local_cache.clone());
    crate::route::set_url(ps,&source.url);
    super::start_bufferfeed(ps,pa)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn silent_background_declaration_is_bounded_and_does_not_invent_a_lower_frame_rate(){
        let mut source:Source=serde_json::from_str(r#"{"url":"http://192.0.2.1/forest.mp4","videoCodec":"hevc","fps":60,"width":3840,"height":2160}"#).unwrap();
        assert!(source.valid());source.fps=240.0;assert!(!source.valid());
        source.fps=60.0;source.video_codec="h264".into();assert!(!source.valid());
        source.fps=30.0;assert!(source.valid());
    }
    #[test]
    fn local_cache_is_opt_in_and_never_turns_url_validation_into_file_access(){
        let mut source:Source=serde_json::from_str(r#"{"url":"https://example.invalid/forest.mp4","videoCodec":"hevc","fps":30,"width":3840,"height":2160,"localCache":{"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","bytes":100}}"#).unwrap();
        assert!(source.valid());source.url="file:///etc/passwd".into();assert!(!source.valid());
        source.url="https://example.invalid/forest.mp4".into();source.local_cache.as_mut().unwrap().bytes=crate::local_cache::MAX_BYTES+1;assert!(!source.valid());
    }
}
