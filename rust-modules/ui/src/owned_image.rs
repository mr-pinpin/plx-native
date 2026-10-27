//! Small immutable application artwork, decoded by its owner before reaching the UI.
//! Pixel data is a render resource, never part of a logical-state fingerprint.
use std::sync::Arc;
use crate::{Painter,Rect,frame::{Budget,Class}};
#[derive(Clone,PartialEq,Eq)]
pub struct Pixels {pub width:u16,pub height:u16,pub rgba:Arc<[u8]>}
impl Pixels {
    pub fn new(width:u16,height:u16,rgba:Vec<u8>)->Option<Self>{
        Self::bounded(width,height,rgba,512,512)
    }
    /// Full-resolution artwork for the 1920×1080 graphics surface. Small app icons retain
    /// their stricter constructor; a background cannot silently raise every icon's limits.
    pub fn backdrop(width:u16,height:u16,rgba:Vec<u8>)->Option<Self>{
        Self::bounded(width,height,rgba,1920,1080)
    }
    fn bounded(width:u16,height:u16,rgba:Vec<u8>,max_width:u16,max_height:u16)->Option<Self>{
        (width>0&&height>0&&width<=max_width&&height<=max_height&&rgba.len()==usize::from(width)*usize::from(height)*4)
            .then(||Self{width,height,rgba:rgba.into()})
    }
}
#[derive(Default)]
pub struct OwnedImage {texture:u32,bytes:usize}
impl OwnedImage {
    pub fn prepare(&mut self,pixels:&Pixels,budget:&mut Budget){
        let class=if pixels.width>512||pixels.height>512{Class::Residency}else{Class::Poster};
        if self.texture==0&&budget.take(class,plx_base::diag::heartbeat::now_us()){
            self.texture=plx_gfx::img::img_upload_rgba(pixels.rgba.as_ptr(),pixels.width.into(),pixels.height.into());
            if self.texture!=0 {self.bytes=pixels.rgba.len();plx_gfx::gfx::warm_tex(self.texture);}
        }
    }
    pub fn render_report(&self)->crate::frame::RenderReport{crate::frame::RenderReport{textures:u32::from(self.ready()),bytes:self.bytes}}
    pub fn ready(&self)->bool{self.texture!=0}
    pub fn draw(&self,painter:Painter,rect:Rect,radius:f32){
        if self.ready(){painter.tex(self.texture,rect,radius,crate::theme::TINT_WHITE);}
    }
}
impl Drop for OwnedImage {fn drop(&mut self){plx_gfx::gfx::delete_tex(self.texture);}}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn a_full_hd_cover_keeps_each_pixel_and_does_not_expand_icon_or_allocation_limits(){
        let rgba=vec![127;1920*1080*4];
        let cover=Pixels::backdrop(1920,1080,rgba.clone()).unwrap();
        assert_eq!((cover.width,cover.height),(1920,1080));assert_eq!(cover.rgba.as_ref(),rgba);
        assert!(Pixels::new(1920,1080,rgba).is_none());
        assert!(Pixels::backdrop(1921,1080,Vec::new()).is_none());
        assert!(Pixels::backdrop(1920,1081,Vec::new()).is_none());
        assert!(Pixels::backdrop(u16::MAX,u16::MAX,Vec::new()).is_none());
        assert!(Pixels::backdrop(1920,1080,vec![0;4]).is_none());
    }
}
