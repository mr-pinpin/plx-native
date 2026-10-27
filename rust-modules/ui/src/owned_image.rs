//! Small immutable application artwork, decoded by its owner before reaching the UI.
//! Pixel data is a render resource, never part of a logical-state fingerprint.
use std::sync::Arc;
use crate::{Painter,Rect,frame::{Budget,Class}};
#[derive(Clone,PartialEq,Eq)]
pub struct Pixels {pub width:u16,pub height:u16,pub rgba:Arc<[u8]>}
impl Pixels {
    pub fn new(width:u16,height:u16,rgba:Vec<u8>)->Option<Self>{
        (width>0&&height>0&&width<=512&&height<=512&&rgba.len()==usize::from(width)*usize::from(height)*4)
            .then(||Self{width,height,rgba:rgba.into()})
    }
}
#[derive(Default)]
pub struct OwnedImage {texture:u32,bytes:usize}
impl OwnedImage {
    pub fn prepare(&mut self,pixels:&Pixels,budget:&mut Budget){
        if self.texture==0&&budget.take(Class::Poster,plx_base::diag::heartbeat::now_us()){
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
