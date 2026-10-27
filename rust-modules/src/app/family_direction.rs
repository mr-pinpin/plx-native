//! Select a pre-encoded direction segment; native Play owns every frame between flips.
#[derive(Debug,PartialEq)]
pub(super) enum Action{None,Pause,Resume,Seek(i64)}
#[derive(Default)]
pub(super) struct Direction{last:Option<[f32;2]>,holding:bool,rate_milli:u16,reverse:bool,desired_reverse:bool}
impl Direction{
    pub fn pointer(&mut self,point:Option<[f32;2]>,_now:u32){
        let Some(point)=point.filter(|p|p.iter().all(|v|v.is_finite()&&(0.0..=1.0).contains(v)))else{self.release();return;};
        let offset=point[0]-0.5;let distance=offset.abs();
        self.holding=if self.holding{distance<0.08}else{distance<=0.06};
        if !self.holding{
            self.desired_reverse=offset<0.0;
            self.rate_milli=(250.0+1750.0*((distance-0.06)/0.44).clamp(0.0,1.0)).round() as u16;
        }
        self.last=Some(point);
    }
    pub fn release(&mut self){self.last=None;self.holding=false;self.rate_milli=1000;}
    pub fn rate_milli(&self,_now:u32)->u16{if self.last.is_some()&&!self.holding{self.rate_milli}else{1000}}
    pub fn owned(&self,_now:u32)->bool{self.last.is_some()}
    pub fn step(&mut self,_now:u32,ready:bool,paused:bool,position:i64,duration:i64,fps:f64)->Action{
        if !ready||position<0||duration<=8_000_000_000||!fps.is_finite()||fps<=0.0{return Action::None;}
        if self.holding{return if paused{Action::None}else{Action::Pause};}
        if paused{return Action::Resume;}
        let half=(duration-4_000_000_000)/2;
        let frame=(1_000_000_000.0/fps) as i64;
        if self.reverse!=self.desired_reverse{
            self.reverse=self.desired_reverse;
            return Action::Seek((2*half-position-frame).clamp(0,2*half-frame));
        }
        let end=if self.reverse{2*half}else{half};
        if position>=end-frame{return Action::Seek(if self.reverse{half}else{0});}
        Action::None
    }
}
#[cfg(test)]mod tests{
 use super::*;
 const SECOND:i64=1_000_000_000;
 fn step(d:&mut Direction,now:u32,pos:i64)->Action{d.step(now,true,false,pos,244*SECOND,30.0)}
 #[test]fn stationary_left_position_selects_reverse_once_and_keeps_playing(){
  let mut d=Direction::default();d.pointer(Some([0.2,0.5]),0);
  assert_eq!(step(&mut d,0,30*SECOND),Action::Seek(210*SECOND-SECOND/30));
  for n in 1..60{d.pointer(Some([0.2,0.5]),n*100);assert_eq!(step(&mut d,n*100,211*SECOND),Action::None);}
  assert!(d.owned(99999));
 }
 #[test]fn absolute_position_not_movement_direction_owns_the_selected_segment(){
  let mut d=Direction::default();d.pointer(Some([0.2,0.5]),0);let _=step(&mut d,0,20*SECOND);
  d.pointer(Some([0.3,0.5]),1);assert_eq!(step(&mut d,1,221*SECOND),Action::None,"rightward movement inside leftside staysreverse");
  d.pointer(Some([0.8,0.5]),2);assert_eq!(step(&mut d,2,222*SECOND),Action::Seek(18*SECOND-SECOND/30));
 }
 #[test]fn center_hold_has_stable_hysteresis_and_leaves_only_outside_outer_zone(){
  let mut d=Direction::default();d.pointer(Some([0.5,0.5]),0);assert_eq!(step(&mut d,0,20*SECOND),Action::Pause);
  for x in [0.49,0.51,0.57,0.43]{d.pointer(Some([x,0.5]),1);assert_eq!(d.step(1,true,true,20*SECOND,244*SECOND,30.0),Action::None);}
  d.pointer(Some([0.59,0.5]),2);assert_eq!(d.step(2,true,true,20*SECOND,244*SECOND,30.0),Action::Resume);
  d.pointer(Some([0.57,0.5]),3);assert_eq!(step(&mut d,3,21*SECOND),Action::None);
 }
 #[test]fn busy_input_coalesces_to_the_latest_absolute_position(){
  let mut d=Direction::default();d.pointer(Some([0.2,0.5]),0);assert_eq!(d.step(0,false,false,20*SECOND,244*SECOND,30.0),Action::None);
  d.pointer(Some([0.8,0.5]),1);assert_eq!(step(&mut d,1,20*SECOND),Action::None);
 }
 #[test]fn each_segment_loops_in_place_and_release_restores_ambient_without_flipping(){
  let mut d=Direction::default();assert_eq!(step(&mut d,0,120*SECOND),Action::Seek(0));
  d.pointer(Some([0.2,0.5]),0);let _=step(&mut d,0,30*SECOND);d.release();assert_eq!(step(&mut d,5000,240*SECOND),Action::Seek(120*SECOND));assert_eq!(d.rate_milli(5000),1000);
 }
}
/// Per-Load optional capability: one refusal disables attempts until a different Load owns it.
pub(super) struct RateOwner<Attempt>{owner:Option<Attempt>,applied:u16,unavailable:bool,called_at:Option<u32>}
impl<Attempt> Default for RateOwner<Attempt>{fn default()->Self{Self{owner:None,applied:1000,unavailable:false,called_at:None}}}
impl<Attempt:Copy+PartialEq> RateOwner<Attempt>{
 pub fn plan(&mut self,owner:Option<Attempt>,desired:u16,now:u32)->Option<u16>{
  if self.owner!=owner{*self=Self{owner,..Default::default()};}
  if owner.is_none()||self.unavailable||!(250..=2000).contains(&desired)||desired==self.applied{return None;}
  if self.called_at.is_some_and(|at|now.wrapping_sub(at)<100){return None;}
  if desired!=1000&&self.applied.abs_diff(desired)<100{return None;}
  let next=if desired==1000||self.applied==0{desired}else{desired.clamp(self.applied.saturating_sub(250),self.applied.saturating_add(250))};
  self.called_at=Some(now);Some(next)
 }
 pub fn invalidate_after_seek(&mut self){self.applied=0;self.called_at=None;}
 pub fn acknowledge(&mut self,rate:u16,accepted:bool){if accepted{self.applied=rate;}else{self.unavailable=true;}}
}
#[cfg(test)]mod positive_rate_tests{
 use super::*;
 #[test]fn edge_distance_sets_magnitude_independent_of_event_speed(){
  let mut inner=Direction::default();inner.pointer(Some([0.65,0.5]),10);
  let mut edge=Direction::default();edge.pointer(Some([1.0,0.5]),100000);
  assert!(edge.rate_milli(100000)>inner.rate_milli(10));assert_eq!(edge.rate_milli(999999),2000);
  edge.pointer(Some([0.0,0.5]),100001);assert_eq!(edge.rate_milli(100001),2000);
  edge.release();assert_eq!(edge.rate_milli(100002),1000);
 }
 #[test]fn refusal_is_load_scoped_and_successful_rates_are_coalesced(){
  let mut r=RateOwner::default();assert_eq!(r.plan(Some(1),1500,0),Some(1250));r.acknowledge(1250,false);
  assert_eq!(r.plan(Some(1),2000,1000),None);assert_eq!(r.plan(Some(2),1500,1001),Some(1250));r.acknowledge(1250,true);
  assert_eq!(r.plan(Some(2),2000,1050),None);assert_eq!(r.plan(Some(2),1300,1200),None);assert_eq!(r.plan(Some(2),1000,1200),Some(1000));r.acknowledge(1000,true);assert_eq!(r.plan(Some(2),1000,1400),None);
  r.invalidate_after_seek();assert_eq!(r.plan(Some(2),1000,1401),Some(1000));r.acknowledge(1000,true);assert_eq!(r.plan(None,2000,1500),None);
 }
}
