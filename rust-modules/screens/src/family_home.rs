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

const TILE_W: f32 = 192.0;
const TILE_H: f32 = 192.0;
const BOTTOM_INSET: f32 = 96.0;
const IDLE_MS: u32 = 8000;
const GROUP: GroupId = GroupId(0);

pub struct FamilyHomeScreen {
    entry: EntryId,
    apps: Vec<Application>,
    idle_ms: u32,
    alpha: Spring,
    pop: CtlPop<5>,
    icons: Vec<plx_ui::owned_image::OwnedImage>,
}
impl FamilyHomeScreen {
    pub fn new(entry: EntryId, apps: Vec<Application>) -> Self {
        let icons=(0..apps.len()).map(|_|Default::default()).collect();
        Self { entry, apps, icons, idle_ms: 0, alpha: Spring::at(1.0), pop: CtlPop::new() }
    }
    fn rect(&self, index: usize) -> Option<Rect> {
        let widths = [Item::new(TILE_W); 5];
        let row = HStack::new(&widths[..self.apps.len().min(5)], theme::space::MD, Align::Centre(plx_ui::consts::SCR_W / 2.0));
        let (x,w) = row.span(index)?;
        Some(Rect::new(x, plx_ui::consts::SCR_H - BOTTOM_INSET - TILE_H, w, TILE_H))
    }
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
        c.seq(self.apps.len());
        for app in &self.apps { c.str(&app.id).str(&app.title); }
    }
    fn probe(&self, out: &mut String) { out.push_str(&format!("family_home apps={} idle_ms={}",self.apps.len(),self.idle_ms)); }
}
impl<H: AppLike> Machine<H> for FamilyHomeScreen {
    type Ev = ScreenEvent<H>;
    fn step(&mut self, ev: &Self::Ev, cx: &Cx<'_,H>, fx: &mut Effects<'_,H>) -> Handled {
        match ev {
            ScreenEvent::Tick(t) => {
                self.idle_ms = self.idle_ms.saturating_add(t.dt_us / 1000);
                let old = self.alpha.pos;
                self.alpha.step(if self.idle_ms < IDLE_MS {1.0}else{0.0},plx_ui::consts::K_SCALE,t.dt());
                self.pop.step(cx.focus.current.filter(|k|k.entry==self.entry).map(|k|k.elem as usize),t.dt());
                if self.idle_ms < IDLE_MS || (old-self.alpha.pos).abs()>0.0001 { fx.invalidate(Provenance::Lifecycle); }
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
    fn render(&self)->RenderStrategy{RenderStrategy::Page}
    fn focus_source(&self)->FocusSource{FocusSource::Engine}
    fn hit_source(&self)->HitSource{HitSource::Engine}
    fn as_any(&self)->Option<&dyn std::any::Any>{Some(self)}
    fn as_any_mut(&mut self)->Option<&mut dyn std::any::Any>{Some(self)}
}
