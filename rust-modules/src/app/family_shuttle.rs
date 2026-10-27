//! Pure pointer-to-timeline policy. The existing frame owner executes its pause/seek/resume
//! intents; no codec rate or undocumented reverse-playback verb is introduced.
// Match the existing native pointer's three-second idle visibility window.
const STALE_MS: u32 = 3000;
const SEEK_MS: u32 = 250;
const DEAD_ZONE: f64 = 0.06;
const MAX_SPEED: f64 = 8.0;
#[derive(Debug, PartialEq)]
pub(super) enum Action {
    None,
    Pause,
    Resume,
    Seek(i64),
}
#[derive(Default)]
pub(super) struct Shuttle {
    pointer: Option<(f32, u32)>,
    last_tick: Option<u32>,
    last_seek: Option<u32>,
    last_pause: Option<u32>,
    last_resume: Option<u32>,
    target: Option<f64>,
    holding: bool,
}
impl Shuttle {
    pub fn pointer(&mut self, x: Option<f32>, now: u32) {
        self.pointer = x
            .filter(|x| x.is_finite())
            .map(|x| (x.clamp(0.0, 1.0), now));
    }
    pub fn release(&mut self) {
        self.pointer = None;
    }
    pub fn forget(&mut self) {
        *self = Self::default();
    }
    pub fn acknowledge_pause(&mut self, accepted: bool) {
        if !accepted {
            self.holding = false;
        }
    }
    pub fn acknowledge_resume(&mut self, accepted: bool) {
        if accepted {
            self.holding = false;
            self.last_resume = None;
        }
    }
    pub fn step(&mut self, now: u32, ready: bool, position: i64, duration: i64) -> Action {
        let point = self
            .pointer
            .filter(|(_, at)| now.wrapping_sub(*at) <= STALE_MS);
        if point.is_none() || duration <= 0 || position < 0 {
            self.pointer = None;
            self.last_tick = None;
            self.last_seek = None;
            self.target = None;
            if self.holding
                && ready
                && self
                    .last_resume
                    .is_none_or(|last| now.wrapping_sub(last) >= SEEK_MS)
            {
                self.last_resume = Some(now);
                return Action::Resume;
            }
            return Action::None;
        }
        let dt = self
            .last_tick
            .map(|last| now.wrapping_sub(last).min(100))
            .unwrap_or(0) as f64
            / 1000.0;
        self.last_tick = Some(now);
        if !ready {
            self.target = None;
            return Action::None;
        }
        if !self.holding {
            if self
                .last_pause
                .is_some_and(|last| now.wrapping_sub(last) < SEEK_MS)
            {
                return Action::None;
            }
            self.last_pause = Some(now);
            self.holding = true;
            self.target = Some(position as f64);
            return Action::Pause;
        }
        let signed = (point.unwrap().0 as f64 - 0.5) * 2.0;
        if signed.abs() <= DEAD_ZONE {
            self.target = Some(position as f64);
            return Action::None;
        }
        let velocity = signed.signum() * (signed.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE) * MAX_SPEED;
        let target = (self.target.unwrap_or(position as f64) + velocity * dt * 1e9)
            .rem_euclid(duration as f64);
        self.target = Some(target);
        if self
            .last_seek
            .is_some_and(|last| now.wrapping_sub(last) < SEEK_MS)
        {
            return Action::None;
        }
        let target = target.round() as i64 % duration;
        if (target - position).abs() < 40_000_000 {
            return Action::None;
        }
        self.last_seek = Some(now);
        Action::Seek(target)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const SECOND: i64 = 1_000_000_000;
    #[test]
    fn centre_holds_and_release_resumes_ambient() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.5), 1000);
        assert_eq!(p.step(1000, true, 60 * SECOND, 120 * SECOND), Action::Pause);
        assert_eq!(p.step(1100, true, 60 * SECOND, 120 * SECOND), Action::None);
        p.release();
        assert_eq!(
            p.step(1110, true, 60 * SECOND, 120 * SECOND),
            Action::Resume
        );
        p.acknowledge_resume(true);
        assert_eq!(p.step(1120, true, 60 * SECOND, 120 * SECOND), Action::None);
    }
    #[test]
    fn signed_speed_is_proportional_and_requests_are_coalesced() {
        let mut p = Shuttle::default();
        p.pointer(Some(1.0), 1000);
        assert_eq!(p.step(1000, true, 60 * SECOND, 120 * SECOND), Action::Pause);
        let Action::Seek(right) = p.step(1100, true, 60 * SECOND, 120 * SECOND) else {
            panic!("right edge must move forward")
        };
        assert_eq!(right, 60 * SECOND + 800_000_000);
        assert_eq!(p.step(1110, true, right, 120 * SECOND), Action::None);
        let mut left = Shuttle::default();
        left.pointer(Some(0.0), 1000);
        left.step(1000, true, 60 * SECOND, 120 * SECOND);
        assert_eq!(
            left.step(1100, true, 60 * SECOND, 120 * SECOND),
            Action::Seek(60 * SECOND - 800_000_000)
        );
    }
    #[test]
    fn busy_seek_cannot_accumulate_a_parked_clock_jump() {
        let mut p = Shuttle::default();
        p.pointer(Some(1.0), 1000);
        p.step(1000, true, 60 * SECOND, 120 * SECOND);
        assert_eq!(p.step(1100, false, 60 * SECOND, 120 * SECOND), Action::None);
        p.pointer(Some(1.0), 10000);
        let Action::Seek(target) = p.step(10000, true, 60 * SECOND, 120 * SECOND) else {
            panic!("fresh edge")
        };
        assert!(target <= 60 * SECOND + 800_000_000);
    }
    #[test]
    fn stale_hidden_and_scene_release_do_not_leave_a_pause_owned() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.5), 1000);
        p.step(1000, true, 0, 120 * SECOND);
        assert_eq!(p.step(4001, true, 0, 120 * SECOND), Action::Resume);
        p.acknowledge_resume(true);
        p.pointer(Some(0.5), 2000);
        p.step(2000, true, 0, 120 * SECOND);
        p.forget();
        assert_eq!(p.step(2001, true, 0, 120 * SECOND), Action::None);
    }
    #[test]
    fn signed_targets_wrap_and_app_clock_wrap_remains_bounded() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.0), u32::MAX - 50);
        p.step(u32::MAX - 50, true, 100_000_000, 120 * SECOND);
        assert_eq!(
            p.step(49, true, 100_000_000, 120 * SECOND),
            Action::Seek(119 * SECOND + 300_000_000)
        );
    }
    #[test]
    fn release_waits_for_an_inflight_seek_before_resuming() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.5), 1000);
        p.step(1000, true, 0, 120 * SECOND);
        p.release();
        assert_eq!(p.step(1010, false, 0, 120 * SECOND), Action::None);
        assert_eq!(p.step(1300, true, 0, 120 * SECOND), Action::Resume);
    }
    #[test]
    fn refused_resume_retains_ownership_and_retries_until_acknowledged() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.5), 1000);
        p.step(1000, true, 0, 120 * SECOND);
        p.release();
        assert_eq!(p.step(1100, true, 0, 120 * SECOND), Action::Resume);
        p.acknowledge_resume(false);
        assert_eq!(p.step(1200, true, 0, 120 * SECOND), Action::None);
        assert_eq!(p.step(1350, true, 0, 120 * SECOND), Action::Resume);
        p.acknowledge_resume(true);
        assert_eq!(p.step(1600, true, 0, 120 * SECOND), Action::None);
    }
    #[test]
    fn refused_pause_preserves_the_fresh_pointer_for_a_bounded_retry() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.5), 1000);
        assert_eq!(p.step(1000, true, 0, 120 * SECOND), Action::Pause);
        p.acknowledge_pause(false);
        assert_eq!(p.step(1100, true, 0, 120 * SECOND), Action::None);
        assert_eq!(p.step(1250, true, 0, 120 * SECOND), Action::Pause);
    }
}
