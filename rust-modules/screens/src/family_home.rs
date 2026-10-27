//! The household launcher: a shared-control row over the app-owned video plane.
use std::ffi::CString;
use plx_machine::machine::{Canon, Cx, Effects, EntryId, FocusKey, Fx, GroupId, Handled, LogicalState, Machine};
use plx_machine::present::Provenance;
use plx_ui::{Rect, Spring, View, Env, theme};
use plx_ui::hstack::{HStack, Item, Align};
use plx_ui::screen::{Activate, At, AxisMask, Dir, DrawFrame, EdgeRule, ElemKind, FocusSource, Focusable, GroupKind, GroupSpec, HitSource, Hover, Placed, RenderStrategy, Screen, ScreenEvent, Seat, Step, Stop};
use plx_ui::widgets::{Button, ControlGround, CtlPop};
use crate::registry::{AppLike, AppFx, LoopReq};

#[derive(Clone, PartialEq, Eq)]
pub struct Application { pub id: String, pub title: String, pub icon: Option<plx_ui::owned_image::Pixels> }

#[derive(Clone,PartialEq,Eq,Default)]
pub struct Status {pub clock:String,pub date:String,pub network:String}

pub const SHAPE:&str="FamilyHomeV1{entry:u32,idle_ms:u32,alpha:{pos:f32,vel:f32},pop:CtlPop<5>,status:{clock:str,date:str,network:str},apps:seq<{id:str,title:str}>}";

const TILE_W: f32 = 192.0;
const TILE_H: f32 = 192.0;
const BOTTOM_INSET: f32 = 96.0;
const GROUP: GroupId = GroupId(0);

pub struct FamilyHomeScreen {
    entry: EntryId,
    apps: Vec<Application>,
    idle_ms: u32,
    status: Status,
    alpha: Spring,
    pop: CtlPop<5>,
    icons: Vec<plx_ui::owned_image::OwnedImage>,
}
impl FamilyHomeScreen {
    pub fn new(entry: EntryId, apps: Vec<Application>) -> Self {
        let icons=(0..apps.len()).map(|_|Default::default()).collect();
        Self { entry, apps, icons, status:Status::default(), idle_ms: 0, alpha: Spring::at(1.0), pop: CtlPop::new() }
    }
    fn rect(&self, index: usize) -> Option<Rect> { dock_rect(self.apps.len(),index) }
}
fn dock_rect(count:usize,index:usize)->Option<Rect>{
        let widths = [Item::new(TILE_W); 5];
        let row = HStack::new(&widths[..count.min(5)], theme::space::MD, Align::Centre(plx_ui::consts::SCR_W / 2.0));
        let (x,w) = row.span(index)?;
        Some(Rect::new(x, plx_ui::consts::SCR_H - BOTTOM_INSET - TILE_H, w, TILE_H))
}
impl FamilyHomeScreen {
    fn key(&self, i: usize) -> FocusKey<u32> { FocusKey { entry: self.entry, elem: i as u32 } }
    fn activate<H: AppLike>(&self, elem: u32, fx: &mut Effects<'_, H>) -> Handled {
        let Some(_app) = self.apps.get(elem as usize) else { return Handled::No };
        fx.push(Fx::App(AppFx::Loop(LoopReq::FamilyLaunch(elem as u8))));
        Handled::Yes
    }
}
impl LogicalState for FamilyHomeScreen {
    fn write(&self, c: &mut Canon) {
        c.u32(self.entry.0).u32(self.idle_ms).f32(self.alpha.pos).f32(self.alpha.vel);
        self.pop.write_motion(c);
        c.str(&self.status.clock).str(&self.status.date).str(&self.status.network);
        c.seq(self.apps.len());
        for app in &self.apps { c.str(&app.id).str(&app.title); }
    }
    fn probe(&self, out: &mut String) { out.push_str(&format!("family_home apps={} idle_ms={}",self.apps.len(),self.idle_ms)); }
}
impl<H: AppLike> Machine<H> for FamilyHomeScreen {
    type Ev = ScreenEvent<H>;
    fn step(&mut self, ev: &Self::Ev, cx: &Cx<'_,H>, fx: &mut Effects<'_,H>) -> Handled {
        match ev {
            ScreenEvent::Resume | ScreenEvent::App(crate::registry::AppMsg::FamilyWake)=>{self.idle_ms=0;self.alpha=Spring::at(1.0);fx.invalidate(Provenance::Lifecycle);Handled::Yes}
            ScreenEvent::App(crate::registry::AppMsg::FamilyStatus(status))=>{if self.status!=*status{self.status=status.clone();fx.invalidate(Provenance::Lifecycle);}Handled::Yes}
            ScreenEvent::Tick(t) => {
                // A parked frame's elapsed time is not inactivity after returning Home.
                self.idle_ms = self.idle_ms.saturating_add((t.dt_us / 1000).min(250));
                                self.alpha.step(1.0,plx_ui::consts::K_SCALE,t.dt());
                self.pop.step(cx.focus.current.filter(|k|k.entry==self.entry).map(|k|k.elem as usize),t.dt());
                fx.invalidate(Provenance::Lifecycle);
                Handled::Yes
            }
            ScreenEvent::Input(_) | ScreenEvent::FocusMoved {..} => {self.idle_ms=0;fx.invalidate(Provenance::Input);Handled::No}
            ScreenEvent::Activate(elem) => self.activate(*elem,fx),
            ScreenEvent::PressCommit(_) => cx.focus.current.filter(|k|k.entry==self.entry)
                .map_or(Handled::No,|key|self.activate(key.elem,fx)),
            _ => Handled::No,
        }
    }
}
impl<H: AppLike> Focusable<H> for FamilyHomeScreen {
    fn groups(&self,_:&Cx<'_,H>,out:&mut Vec<GroupSpec>) {
        out.push(GroupSpec{id:GROUP,kind:GroupKind::Row{wrap:false},seat:Seat::First,reachable:AxisMask::BOTH,
            edge:[EdgeRule::Stop;4],extent:Rect::FULL,len:self.apps.len(),elem:ElemKind::Control});
    }
    fn group_of(&self,elem:&u32,_:&Cx<'_,H>)->Option<GroupId>{((*elem as usize)<self.apps.len()).then_some(GROUP)}
    fn neighbour(&self,key:FocusKey<u32>,dir:Dir,_:&Cx<'_,H>)->Step<u32>{
        let index=match dir{Dir::Left=>key.elem.checked_sub(1),Dir::Right=>key.elem.checked_add(1),_=>None};
        index.filter(|i|(*i as usize)<self.apps.len()).map_or(Step::Edge,|i|Step::Move(self.key(i as usize)))
    }
    fn place(&self,elem:&u32,_:&Cx<'_,H>,_:At)->Option<Placed>{
        if (*elem as usize)>=self.apps.len(){return None;}
        let rect=self.rect(*elem as usize)?;Some(Placed{rect,rest_rect:rect,clip:Rect::FULL,index:Some(*elem)})
    }
    fn reconcile(&self,key:FocusKey<u32>,_:&Cx<'_,H>)->FocusKey<u32>{self.key((key.elem as usize).min(self.apps.len().saturating_sub(1)))}
    fn seat(&self,_:GroupId,placed:Placed,_:&Cx<'_,H>)->FocusKey<u32>{self.key(placed.index.unwrap_or(0) as usize)}
}
impl<H:AppLike> Screen<H> for FamilyHomeScreen {
    fn name(&self)->&'static str{"family_home"}
    fn state(&self)->&dyn LogicalState{self}
    fn crumb(&self,_:&Cx<'_,H>)->Option<std::borrow::Cow<'_,str>>{None}
    fn prepare(&mut self,budget:&mut plx_ui::frame::Budget,_:&Cx<'_,H>){
        for (app,icon) in self.apps.iter().zip(&mut self.icons){if let Some(pixels)=&app.icon{icon.prepare(pixels,budget);}}
    }
    fn draw(&mut self,f:&mut DrawFrame<'_,'_,H>){
        let p=f.painter.alpha(f.page_alpha*self.alpha.pos.clamp(0.0,1.0));
        if let (Ok(clock),Ok(date))=(CString::new(self.status.clock.as_str()),CString::new(self.status.date.as_str())){
            let text=plx_ui::label::Label::new(clock.as_ptr(),theme::size::BODY,theme::TEXT_PRIMARY);
            text.draw(p,Rect::new(80.0,56.0,300.0,48.0));
            if let Ok(network)=CString::new(self.status.network.as_str()){plx_ui::label::Label::new(network.as_ptr(),theme::size::BODY,theme::TEXT_SECONDARY).h(plx_ui::label::HAlign::Center).draw(p,Rect::new(plx_ui::consts::SCR_W/2.0-240.0,56.0,480.0,48.0));}
            plx_ui::label::Label::new(date.as_ptr(),theme::size::BODY,theme::TEXT_SECONDARY).h(plx_ui::label::HAlign::Right).draw(p,Rect::new(plx_ui::consts::SCR_W-580.0,56.0,500.0,48.0));
        }
        if let (Some(first),Some(last))=(self.rect(0),self.rect(self.apps.len().saturating_sub(1))){
            p.rrect_sheened(Rect::new(first.x-24.0,first.y-24.0,last.x+last.w-first.x+48.0,TILE_H+48.0),32.0,theme::MENU_GLASS_BOT);
        }
        for (i,app) in self.apps.iter().enumerate(){
            let Some(rect)=self.rect(i)else{continue};let Ok(label)=CString::new(app.title.as_str())else{continue};
            let focused=f.cx.focus.current.is_some_and(|k|k==self.key(i));
            if self.icons[i].ready(){
                let popped=rect.scaled(self.pop.scale(i));
                self.icons[i].draw(p,popped,32.0);
                if focused {plx_ui::label::Label::new(label.as_ptr(),theme::size::BODY,theme::TEXT_PRIMARY).h(plx_ui::label::HAlign::Center).draw(p,Rect::new(rect.x-80.0,rect.y-64.0,rect.w+160.0,48.0));}
            }else{
                Button::new(label.as_ptr(),theme::size::BODY,rect).focused(focused).ground(ControlGround::Unkeyed)
                    .scale(self.pop.scale(i)).draw(&Env::inert(),p);
            }
            f.stop(p,Stop{key:self.key(i),rect,rest_rect:rect,clip:Rect::FULL,hover:Hover::Focus,activate:Activate::Press});
        }
    }
    fn render_report(&self)->plx_ui::frame::RenderReport{
        self.icons.iter().map(|icon|icon.render_report()).fold(plx_ui::frame::RenderReport::NONE,|mut total,icon|{total.textures+=icon.textures;total.bytes+=icon.bytes;total})
    }
    fn render(&self)->RenderStrategy{RenderStrategy::VideoPlane}
    fn focus_source(&self)->FocusSource{FocusSource::Engine}
    fn hit_source(&self)->HitSource{HitSource::Engine}
    fn as_any(&self)->Option<&dyn std::any::Any>{Some(self)}
    fn as_any_mut(&mut self)->Option<&mut dyn std::any::Any>{Some(self)}
}

/// Scene input cannot own a dock control or its focus-growth margin.
pub fn scene_point(count:usize,x:f32,y:f32)->Option<[f32;2]>{
    let w=plx_ui::consts::SCR_W;let h=plx_ui::consts::SCR_H;
    if !x.is_finite()||!y.is_finite()||x<0.0||y<0.0||x>w||y>h{return None;}
    if let (Some(first),Some(last))=(dock_rect(count,0),dock_rect(count,count.saturating_sub(1))){
        if x>=first.x-24.0&&x<=last.x+last.w+24.0&&y>=first.y-24.0&&y<=first.y+TILE_H+24.0{return None;}
    }
    Some([x/w,y/h])
}

#[cfg(test)]
mod scene_guard_tests {
    use super::*;
    #[test]
    fn scene_coordinates_never_take_any_visible_dock_control_or_growth_margin(){
        for count in 1..=5 {
            for index in 0..count {let rect=dock_rect(count,index).unwrap();
                assert!(scene_point(count,rect.cx(),rect.y+rect.h/2.0).is_none());
                assert!(scene_point(count,rect.x-20.0,rect.y-20.0).is_none());
            }
            assert_eq!(scene_point(count,plx_ui::consts::SCR_W/2.0,plx_ui::consts::SCR_H/2.0),Some([0.5,0.5]));
        }
        assert!(scene_point(4,f32::NAN,0.0).is_none());assert!(scene_point(4,-1.0,0.0).is_none());
    }
}

#[cfg(test)]
mod resume_tests {
    use super::*;
    use crate::family::InnerHost;
    use plx_machine::machine::{Tick,FocusRead,PressRead,InputOwner,MachineId};
    #[test]
    fn family_resume_shows_the_dock_before_video_and_ignores_the_parked_frame_gap(){
        let mut screen=FamilyHomeScreen::new(EntryId(1),Vec::new());
        screen.idle_ms=31000;screen.alpha=Spring::at(0.0);
        let measure=plx_ui::fixture::FixtureMeasure;
        let cx=Cx::<InnerHost>{views:plx_data::stores::browse::DirectoryView::empty_for_test(),tick:Tick::default(),measure:&measure,press:PressRead::default(),focus:FocusRead::default(),owner:InputOwner::Entry(EntryId(1))};
        let mut effects=Vec::new();let mut present=plx_machine::present::Present::default();
        let mut fx=Effects::new(&mut effects,MachineId::Input,&mut present);
        screen.step(&ScreenEvent::Resume,&cx,&mut fx);
        assert_eq!(screen.idle_ms,0,"returning from Cinema must reset launcher idle");
        assert_eq!(screen.alpha.pos,1.0,"the dock must appear before video buffering finishes");
        screen.step(&ScreenEvent::Tick(Tick{ms:60000,dt_us:60000000}),&cx,&mut fx);
        assert!(screen.idle_ms<30000,"a parked minute cannot immediately hide the resumed dock");
        assert_eq!(screen.alpha.pos,1.0);
        for tick in 0..1200 {screen.step(&ScreenEvent::Tick(Tick{ms:60000+tick*100,dt_us:100000}),&cx,&mut fx);}
        assert_eq!(screen.alpha.pos,1.0,"the launcher menu must remain visible at all times");
    }
}
