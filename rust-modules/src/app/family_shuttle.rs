//! Pure pointer-to-timeline policy. The existing frame owner executes its pause/seek/resume
//! intents; no codec rate or undocumented reverse-playback verb is introduced.
// Release only after a complete second without actual two-dimensional motion.
const STALE_MS: u32 = 1000;
const SEEK_MS: u32 = 250;
const CENTRE_ZONE: f64 = 0.06;
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
    pointer: Option<([f32; 2], u32)>,
    last_position: Option<[f32; 2]>,
    last_tick: Option<u32>,
    last_seek: Option<u32>,
    last_pause: Option<u32>,
    last_resume: Option<u32>,
    target: Option<f64>,
    holding: bool,
}
impl Shuttle {
    #[cfg(test)]
    pub fn pointer(&mut self, x: Option<f32>, now: u32) {
        self.pointer_motion(x.map(|x| [x, 0.5]), now);
    }
    pub fn pointer_motion(&mut self, point: Option<[f32; 2]>, now: u32) {
        let Some(point) = point
            .filter(|point| point.iter().all(|v| v.is_finite()))
            .map(|point| [point[0].clamp(0.0, 1.0), point[1].clamp(0.0, 1.0)])
        else {
            self.release();
            return;
        };
        // A stationary packet is not motion and cannot renew an expired ownership lease.
        if self.last_position == Some(point) {
            return;
        }
        let direction = |x: f32| {
            if (x as f64 - 0.5) * 2.0 < -CENTRE_ZONE {
                -1
            } else {
                1
            }
        };
        if self.pointer.is_none_or(|(old, at)| {
            now.wrapping_sub(at) >= STALE_MS || direction(point[0]) != direction(old[0])
        }) {
            self.target = None;
            self.last_tick = Some(now);
        }
        self.last_position = Some(point);
        self.pointer = Some((point, now));
    }
    pub fn desired(&self) -> Option<f64> {
        self.target
    }
    pub fn point(&self, now: u32) -> Option<(f32, u32)> {
        self.pointer
            .map(|(point, at)| (point[0], now.wrapping_sub(at)))
    }
    pub fn holding(&self) -> bool {
        self.holding || self.pointer.is_some()
    }
    pub fn release(&mut self) {
        self.pointer = None;
        self.last_position = None;
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
        let seek_span = plx_appkit::player_hud::scrub_clamp_target(duration, duration);
        let anchor = plx_appkit::player_hud::scrub_clamp_target(position, duration);
        let point = self
            .pointer
            .filter(|(_, at)| now.wrapping_sub(*at) < STALE_MS);
        if point.is_none() || duration <= 0 || seek_span <= 0 || position < 0 {
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
        if !self.holding {
            if !ready {
                return Action::None;
            }
            if self
                .last_pause
                .is_some_and(|last| now.wrapping_sub(last) < SEEK_MS)
            {
                return Action::None;
            }
            self.last_pause = Some(now);
            self.holding = true;
            self.target = Some(anchor as f64);
            return Action::Pause;
        }
        let signed = (point.unwrap().0[0] as f64 - 0.5) * 2.0;
        // The centre advances normally through this same owned timeline. It never
        // releases to a competing ambient clock or freezes a moving gesture.
        let velocity = if signed.abs() <= CENTRE_ZONE {
            1.0
        } else {
            signed.signum() * (signed.abs() - CENTRE_ZONE) / (1.0 - CENTRE_ZONE) * MAX_SPEED
        };
        // Reuse the player's existing priming boundary. Ambient playback may reach
        // natural EOF; a requested seek must never wrap into its unprimeable tail.
        let target = (self.target.unwrap_or(anchor as f64) + velocity * dt * 1e9)
            .rem_euclid(seek_span as f64);
        self.target = Some(target);
        // The desired clock belongs to the live gesture, not the decoder's quantized
        // landed PTS. Keep integrating bounded frame deltas while a seek is in flight;
        // only dispatch waits. Re-anchoring every busy frame reduced an eightfold
        // gesture to one frame's movement and could repeatedly seek the same GOP.
        if !ready {
            return Action::None;
        }
        if self
            .last_seek
            .is_some_and(|last| now.wrapping_sub(last) < SEEK_MS)
        {
            return Action::None;
        }
        let target = target.round() as i64 % seek_span;
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
    fn unchanged_packets_release_after_one_second_and_cannot_reclaim_motion() {
        let mut p = Shuttle::default();
        p.pointer_motion(Some([0.9, 0.5]), 1000);
        p.step(1000, true, 60 * SECOND, 120 * SECOND);
        p.pointer_motion(Some([0.9, 0.5]), 1900);
        assert_ne!(
            p.step(1999, true, 60 * SECOND, 120 * SECOND),
            Action::Resume
        );
        assert_eq!(
            p.step(2000, true, 60 * SECOND, 120 * SECOND),
            Action::Resume
        );
        p.acknowledge_resume(true);
        p.pointer_motion(Some([0.9, 0.5]), 2200);
        assert_eq!(p.step(2200, true, 60 * SECOND, 120 * SECOND), Action::None);
    }
    #[test]
    fn moving_vertically_at_centre_keeps_exclusive_normal_speed() {
        let mut p = Shuttle::default();
        p.pointer_motion(Some([0.5, 0.4]), 1000);
        assert_eq!(p.step(1000, true, 60 * SECOND, 120 * SECOND), Action::Pause);
        p.pointer_motion(Some([0.5, 0.6]), 1050);
        assert_eq!(
            p.step(1100, true, 60 * SECOND, 120 * SECOND),
            Action::Seek(60 * SECOND + 100_000_000)
        );
        p.pointer_motion(Some([0.5, 0.7]), 1900);
        assert_ne!(
            p.step(2100, true, 60 * SECOND, 120 * SECOND),
            Action::Resume
        );
    }
    #[test]
    fn motion_owns_the_timeline_before_the_pause_actuator_is_ready() {
        let mut p = Shuttle::default();
        p.pointer_motion(Some([0.2, 0.5]), 1000);
        assert_eq!(p.step(1000, false, 60 * SECOND, 120 * SECOND), Action::None);
        assert!(p.holding());
    }
    #[test]
    fn continuous_xy_motion_never_releases_and_pending_resume_still_owns() {
        let mut p = Shuttle::default();
        for now in (0..=5000).step_by(100) {
            p.pointer_motion(Some([0.5, if now % 200 == 0 { 0.4 } else { 0.6 }]), now);
            assert_ne!(p.step(now, true, 60 * SECOND, 120 * SECOND), Action::Resume);
            assert!(p.holding());
        }
        assert_ne!(
            p.step(5999, true, 60 * SECOND, 120 * SECOND),
            Action::Resume
        );
        assert_eq!(p.step(6000, false, 60 * SECOND, 120 * SECOND), Action::None);
        assert!(p.holding());
        assert_eq!(
            p.step(6010, true, 60 * SECOND, 120 * SECOND),
            Action::Resume
        );
        assert!(p.holding());
        p.acknowledge_resume(false);
        assert!(p.holding());
        assert_eq!(
            p.step(6260, true, 60 * SECOND, 120 * SECOND),
            Action::Resume
        );
        p.acknowledge_resume(true);
        assert!(!p.holding());
    }
    #[test]
    fn reverse_wrap_never_enters_the_shared_unprimeable_tail() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.0), 1000);
        p.step(1000, true, 100_000_000, 120 * SECOND);
        let Action::Seek(target) = p.step(1100, true, 100_000_000, 120 * SECOND) else {
            panic!("reverse wrap")
        };
        let safe_end = plx_appkit::player_hud::scrub_clamp_target(120 * SECOND, 120 * SECOND);
        assert!(
            target >= 0 && target <= safe_end,
            "seek target {target} exceeds shared safe end {safe_end}"
        );
    }
    #[test]
    fn engaging_in_the_ambient_tail_reanchors_to_the_existing_safe_boundary() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.0), 1000);
        p.step(1000, true, 118 * SECOND, 120 * SECOND);
        let cap = plx_appkit::player_hud::scrub_clamp_target(120 * SECOND, 120 * SECOND);
        assert_eq!(
            p.step(1100, true, 118 * SECOND, 120 * SECOND),
            Action::Seek(cap - 800_000_000)
        );
    }
    #[test]
    fn active_pointer_keeps_its_virtual_timeline_while_decoder_is_busy() {
        let mut p = Shuttle::default();
        p.pointer(Some(1.0), 1000);
        p.step(1000, true, 60 * SECOND, 120 * SECOND);
        for now in [1100, 1200, 1300, 1400, 1500] {
            assert_eq!(p.step(now, false, 60 * SECOND, 120 * SECOND), Action::None);
        }
        assert_eq!(
            p.step(1600, true, 60 * SECOND, 120 * SECOND),
            Action::Seek(64 * SECOND + 800_000_000)
        );
    }
    #[test]
    fn reversing_pointer_direction_reanchors_instead_of_replaying_old_backlog() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.0), 1000);
        p.step(1000, true, 60 * SECOND, 120 * SECOND);
        p.step(1100, true, 60 * SECOND, 120 * SECOND);
        for now in [1200, 1300, 1400] {
            p.step(now, false, 59 * SECOND, 120 * SECOND);
        }
        p.pointer(Some(1.0), 1450);
        assert_eq!(
            p.step(1500, true, 59 * SECOND, 120 * SECOND),
            Action::Seek(59 * SECOND + 400_000_000)
        );
    }
    #[test]
    fn centre_advances_exclusively_and_release_resumes_ambient() {
        let mut p = Shuttle::default();
        p.pointer(Some(0.5), 1000);
        assert_eq!(p.step(1000, true, 60 * SECOND, 120 * SECOND), Action::Pause);
        assert_eq!(
            p.step(1100, true, 60 * SECOND, 120 * SECOND),
            Action::Seek(60 * SECOND + 100_000_000)
        );
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
        assert_eq!(
            p.step(10000, true, 60 * SECOND, 120 * SECOND),
            Action::Resume
        );
        p.acknowledge_resume(true);
        p.pointer(Some(0.9), 10100);
        assert_eq!(
            p.step(10100, true, 60 * SECOND, 120 * SECOND),
            Action::Pause
        );
        let Action::Seek(target) = p.step(10200, true, 60 * SECOND, 120 * SECOND) else {
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
            Action::Seek(
                plx_appkit::player_hud::scrub_clamp_target(120 * SECOND, 120 * SECOND)
                    - 700_000_000
            )
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
