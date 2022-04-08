use std::convert::TryInto;
use std::time::{Duration, Instant};

/**

Given a stream of times from some remote device, calculate the best-estimate of
the corresponding local times.

This assumes that the remote clock proceeds at a consistent rate, but does NOT assume
that it is at the same rate as the local clock, or even what units it uses
(for example, "remote times" could be indices of reports that occur at regular intervals).
Instead, it only assumes that
1) the remote clock is regular (does not change speed)
2) messages cannot arrive before they are sent, and
3) messages will at least occasionally arrive promptly.

All operations are O(1) time complexity, and if messages
frequently arrive less than `tolerance` after they are sent,
the data structure uses only O(1) space.

*/

pub struct RemoteTimeEstimator {
    tolerance: Duration,
    frontier: Vec<(f64, Instant)>,
    before_middle_index: usize,
}

impl Default for RemoteTimeEstimator {
    fn default() -> Self {
        Self::new(Duration::from_micros(200))
    }
}

impl RemoteTimeEstimator {
    pub fn new(tolerance: Duration) -> Self {
        RemoteTimeEstimator {
            tolerance,
            frontier: Vec::new(),
            before_middle_index: 0,
        }
    }

    pub fn observe(&mut self, remote_time: f64, received_by: Instant) {
        if let Some(&(last_remote, last_received)) = self.frontier.last() {
            assert!(
                remote_time >= last_remote,
                "RemoteTimeEstimator assumes observations will be received in-order by remote time"
            );
            assert!(
                received_by >= last_received,
                "RemoteTimeEstimator assumes observations will be received in-order by local received time"
            );
            if remote_time == last_remote {
                // receiving the same thing again but later tells us nothing,
                // and may cause division by zero below
                return;
            }
        }

        while let Some(a_idx) = self.frontier.len().checked_sub(2) {
            let &[(ar, al), (br, bl)]: &[_; 2] = (&self.frontier[a_idx..]).try_into().unwrap();
            let bl_relative = (bl - al).as_secs_f64();
            let bl_relative_estimated =
                (br - ar) * (received_by - al).as_secs_f64() / (remote_time - ar);
            // if the middle one arrived late, it can't be part of the frontier
            if bl_relative >= bl_relative_estimated - self.tolerance.as_secs_f64() {
                self.frontier.pop();
            } else {
                break;
            }
        }

        if let Some(&(first_remote, _)) = self.frontier.first() {
            let middle_remote = (remote_time + first_remote) * 0.5;
            while self
                .frontier
                .get(self.before_middle_index + 1)
                .map_or(false, |&(r, _)| r < middle_remote)
            {
                self.before_middle_index += 1
            }
        }

        self.frontier.push((remote_time, received_by));
    }

    /// Give the most up-to-date estimate of the local time corresponding to a fixed
    /// remote time which has been observed.
    ///
    /// May give bad answers or panic if you pass a remote that has not been observed.
    pub fn estimate_local_time(&self, remote_time: f64) -> Instant {
        if let Some(slice) = self
            .frontier
            .get(self.before_middle_index..self.before_middle_index + 2)
        {
            let &[(ar, al), (br, bl)]: &[_; 2] = slice.try_into().unwrap();
            al + Duration::from_secs_f64((bl - al).as_secs_f64() * (remote_time - ar) / (br - ar))
        } else if let Some(&(_, first_local)) = self.frontier.first() {
            first_local
        } else {
            panic!("estimate_local_time should only be called after there is at least 1 sample")
        }
    }
}
