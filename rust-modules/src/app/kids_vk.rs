//! Cinema Kids' "VK Video" action: open the installed native VK Video application.
//!
//! The target is a compile-time constant ([`APP_ID`]); nothing a screen, a setting, a launch
//! parameter or a deeplink says can change which application the press launches. Only the Kids
//! install carries this state at all (`Bridge::enable_kids_video`), so a request on any other
//! install is dropped here rather than trusted to the screens.
//!
//! The launch is the platform's existing system launch seam (`plx_platform::tv::launch`); its graded
//! reply comes back on the frame thread through [`KidsVideo::poll`]. A press while the previous
//! one is still in flight does nothing. A failed reply is told with the television's own toast
//! (`plx_platform::tv::toast`, as `clock_notice` does), and nothing else moves: the action that was
//! pressed keeps focus, so OK asks again.

/// The VK Video application's id on the television. A constant, never configuration.
pub(crate) const APP_ID: &str = "vkvideo";

#[derive(Default)]
pub(crate) struct KidsVideo {
    /// A launch was requested and its reply has not come back.
    pending: bool,
}

impl KidsVideo {
    /// The action was pressed. `true` when a launch request was made; `false` while the previous
    /// one is still in flight.
    pub(crate) fn press(&mut self) -> bool {
        if self.pending { return false; }
        self.pending = true;
        plx_platform::tv::launch::application(APP_ID);
        true
    }

    /// One frame's look at the system's replies. `Some(message)` when VK Video's launch failed:
    /// the words to show. The latch is released either way, so a retry is possible.
    pub(crate) fn poll(&mut self) -> Option<String> {
        let mut failed = false;
        for outcome in plx_platform::tv::launch::take_outcomes() {
            if outcome.id != APP_ID { continue; }
            self.pending = false;
            failed |= !outcome.accepted;
        }
        failed.then(|| plx_platform::i18n::msg::browse_kids_vk_failed().to_owned())
    }

    #[cfg(test)]
    pub(crate) fn pending(&self) -> bool { self.pending }
}

/// Show `message` through the television's own toast, off the frame thread.
pub(crate) fn tell(message: String) {
    plx_base::task::spawn_small("kids video notice", move || {
        let outcome = plx_platform::tv::toast::toast(&message);
        plx_base::eventlog::log(&format!("kids video: failure toast {outcome:?}"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use plx_platform::tv::launch;

    fn fresh() -> KidsVideo {
        let _ = launch::take_outcomes();
        let _ = launch::take_requested_for_test();
        KidsVideo::default()
    }

    #[test]
    fn pressing_requests_exactly_vkvideo() {
        let _serial = plx_base::testlock::serial();
        let mut vk = fresh();
        assert!(vk.press());
        assert_eq!(launch::take_requested_for_test(), vec!["vkvideo".to_owned()]);
    }

    #[test]
    fn a_press_while_the_request_is_in_flight_is_ignored() {
        let _serial = plx_base::testlock::serial();
        let mut vk = fresh();
        assert!(vk.press());
        assert!(!vk.press(), "second press does nothing");
        assert!(!vk.press());
        assert_eq!(launch::take_requested_for_test(), vec!["vkvideo".to_owned()], "one request only");
        assert!(vk.pending());
    }

    #[test]
    fn an_accepted_reply_releases_the_latch_without_a_message() {
        let _serial = plx_base::testlock::serial();
        let mut vk = fresh();
        vk.press();
        launch::report(APP_ID, true);
        assert_eq!(vk.poll(), None);
        assert!(!vk.pending());
    }

    #[test]
    fn a_failed_reply_says_so_and_a_retry_requests_again() {
        let _serial = plx_base::testlock::serial();
        let mut vk = fresh();
        vk.press();
        assert_eq!(vk.poll(), None, "nothing has come back yet");
        launch::report(APP_ID, false);
        let message = vk.poll().expect("a failed launch is told");
        assert!(message.contains("VK Video"), "{message}");
        assert!(!vk.pending());
        let _ = launch::take_requested_for_test();
        assert!(vk.press(), "OK retries");
        assert_eq!(launch::take_requested_for_test(), vec!["vkvideo".to_owned()]);
    }

    #[test]
    fn another_applications_reply_is_not_ours() {
        let _serial = plx_base::testlock::serial();
        let mut vk = fresh();
        vk.press();
        launch::report("com.example.other", false);
        assert_eq!(vk.poll(), None);
        assert!(vk.pending(), "still waiting for vkvideo's own reply");
    }

    #[test]
    fn a_non_kids_bridge_drops_the_request_and_a_kids_bridge_runs_the_whole_cycle() {
        let _serial = plx_base::testlock::serial();
        let _ = launch::take_outcomes();
        let _ = launch::take_requested_for_test();
        let mut parents = super::super::bridge::Bridge::for_test(|| 0);
        assert!(!parents.press_kids_video(), "no VK Video state on Parents/stable/debug");
        assert!(launch::take_requested_for_test().is_empty(), "nothing is launched");
        assert_eq!(parents.poll_kids_video(), None);

        let mut kids = super::super::bridge::Bridge::for_test(|| 0);
        kids.enable_kids_video(true);
        assert!(kids.press_kids_video());
        assert!(!kids.press_kids_video(), "pending: ignored");
        assert_eq!(launch::take_requested_for_test(), vec!["vkvideo".to_owned()]);
        launch::report("vkvideo", false);
        assert!(kids.poll_kids_video().is_some_and(|m| m.contains("VK Video")));
        assert!(kids.press_kids_video(), "the retry goes through");
        assert_eq!(launch::take_requested_for_test(), vec!["vkvideo".to_owned()]);
        launch::report("vkvideo", true);
        assert_eq!(kids.poll_kids_video(), None);
    }
}
