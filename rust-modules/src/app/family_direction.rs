//! Select a pre-encoded direction segment; native Play owns every frame between flips.
const MOTION_THRESHOLD:f32=0.005;
const IDLE_MS:u32=1000;
#[derive(Debug,PartialEq)]
pub(super) enum Action{None,Resume,Seek(i64)}
#[derive(Default)]
pub(super) struct Direction{last:Option<[f32;2]>,motion_at:Option<u32>,delta:f32,reverse:bool,desired_reverse:bool}
impl Direction{
    pub fn pointer(&mut self,point:Option<[f32;2]>,now:u32){
        let Some(point)=point.filter(|p|p.iter().all(|v|v.is_finite()))else{self.release();return;};
        if let Some(old)=self.last{
            if point!=old{self.motion_at=Some(now);}
            self.delta=(self.delta+point[0]-old[0]).clamp(-1.0,1.0);
            if self.delta.abs()>=MOTION_THRESHOLD{self.desired_reverse=self.delta<0.0;self.delta=0.0;}
        }
        self.last=Some(point);
    }
    pub fn release(&mut self){self.last=None;self.motion_at=None;self.delta=0.0;}
    pub fn owned(&self,now:u32)->bool{self.motion_at.is_some_and(|at|now.wrapping_sub(at)<IDLE_MS)}
    pub fn step(&mut self,now:u32,ready:bool,paused:bool,position:i64,duration:i64,fps:f64)->Action{
        if !self.owned(now){self.motion_at=None;}
        if !ready||position<0||duration<=8_000_000_000||!fps.is_finite()||fps<=0.0{return Action::None;}
        if paused{return Action::Resume;}
        let half=(duration-4_000_000_000)/2;
        let frame=(1_000_000_000.0/fps) as i64;
        if self.reverse!=self.desired_reverse{
            self.reverse=self.desired_reverse;
            return Action::Seek((2*half-position-frame).clamp(0,2*half-frame));
        }
        // The final four seconds are priming padding, never part of either segment.
        let end=if self.reverse{2*half}else{half};
        if position>=end-frame{return Action::Seek(if self.reverse{half}else{0});}
        Action::None
    }
}
#[cfg(test)]
mod tests{
 use super::*;
 const SECOND:i64=1_000_000_000;
 fn step(d:&mut Direction,now:u32,pos:i64)->Action{d.step(now,true,false,pos,244*SECOND,30.0)}
 #[test]fn continuous_motion_never_pauses_or_reseeks_after_one_direction_flip(){
  let mut d=Direction::default();d.pointer(Some([0.8,0.5]),0);d.pointer(Some([0.79,0.5]),10);
  assert_eq!(step(&mut d,10,30*SECOND),Action::Seek(210*SECOND-SECOND/30));
  for n in 1..60{d.pointer(Some([0.79-n as f32*0.001,0.5]),10+n*10);assert_eq!(step(&mut d,10+n*10,211*SECOND),Action::None);}
 }
 #[test]fn sign_flips_use_latest_landed_pts_and_busy_requests_coalesce(){
  let mut d=Direction::default();d.pointer(Some([0.5,0.5]),0);d.pointer(Some([0.4,0.5]),1);
  assert_eq!(d.step(1,false,false,20*SECOND,244*SECOND,30.0),Action::None);
  d.pointer(Some([0.6,0.5]),2);assert_eq!(step(&mut d,2,21*SECOND),Action::None);
  d.pointer(Some([0.5,0.5]),3);assert!(matches!(step(&mut d,3,22*SECOND),Action::Seek(_)));
  d.pointer(Some([0.6,0.5]),4);assert_eq!(step(&mut d,4,218*SECOND),Action::Seek(22*SECOND-SECOND/30));
 }
 #[test]fn stationary_jitter_vertical_and_idle_keep_last_play_direction(){
  let mut d=Direction::default();d.pointer(Some([0.5,0.5]),0);
  for n in 1..40{d.pointer(Some([if n%2==0{0.5}else{0.501},0.5+n as f32*0.001]),n);assert_eq!(step(&mut d,n,20*SECOND),Action::None);}
  assert!(d.owned(1038));assert!(!d.owned(1039));assert_eq!(step(&mut d,2000,21*SECOND),Action::None);
  d.release();assert_eq!(step(&mut d,3000,22*SECOND),Action::None);
 }
 #[test]fn each_segment_loops_in_place_without_idle_direction_reset(){
  let mut d=Direction::default();assert_eq!(step(&mut d,0,120*SECOND),Action::Seek(0));
  d.pointer(Some([0.5,0.5]),0);d.pointer(Some([0.4,0.5]),1);let _=step(&mut d,1,30*SECOND);
  d.release();assert_eq!(step(&mut d,5000,240*SECOND),Action::Seek(120*SECOND));
  assert_eq!(step(&mut d,5001,121*SECOND),Action::None);
 }
 #[test]fn paused_legacy_state_resumes_before_direction_seek(){let mut d=Direction::default();assert_eq!(d.step(0,true,true,0,244*SECOND,30.0),Action::Resume);}
}
