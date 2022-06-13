use crate::webserver::{FrequenciesFrame, HistoryFrame};
// use rustfft::num_complex::Complex;
// use rustfft::FftPlanner;
use crate::utils::get_variable;
use arrayvec::ArrayVec;
use num_complex::Complex;
use statrs::statistics::Statistics;
use std::collections::VecDeque;
use std::f64::consts::TAU;
use std::ops::{AddAssign, Div, SubAssign};
//use std::time::Duration;

// struct Signals {
//     signals: Vec<Signal>,
// }

pub enum ActiveState {
    Active {
        last_sustained_time: f64,
    },
    Inactive {
        last_deactivated_time: f64,
        last_deactivated_sample: i64,
    },
}

#[derive(Default)]
pub struct Window<T, const SIZE: usize> {
    num_values_seen: u64,
    values: VecDeque<T>,
    cached_sum: T,
    next_sum: T,
}

impl<T: Default + Copy + AddAssign + SubAssign, const SIZE: usize> Window<T, SIZE> {
    pub fn push(&mut self, value: T) {
        self.num_values_seen += 1;
        if self.values.len() >= SIZE {
            self.cached_sum -= self.values.pop_front().unwrap();
        }
        self.values.push_back(value);
        self.cached_sum += value;
        self.next_sum += value;
        if self.num_values_seen % SIZE as u64 == 0 {
            self.cached_sum = self.next_sum;
            self.next_sum = T::default();
        }
    }
    // pub fn sum(&self) -> T {
    //     self.cached_sum
    // }
    pub fn values(&self) -> impl Iterator<Item = &T> + '_ {
        self.values.iter()
    }
    pub fn num_values_seen(&self) -> u64 {
        self.num_values_seen
    }
    pub fn last(&self) -> Option<T> {
        self.values.back().copied()
    }
    // pub fn is_full(&self) -> bool {
    //     self.values.len() == SIZE
    // }
}

impl<T: Copy + Div<f64>, const SIZE: usize> Window<T, SIZE> {
    pub fn mean(&self) -> T::Output {
        self.cached_sum / self.values.len() as f64
    }
}

/*

Threshold-setting rule:

When we judge that the signal is idle for 3 seconds, we update the activity threshold based on some statistics about the signal over those 3 seconds.

Caveat: to be judged idle, the signal must not merely "not be explicitly judged active" for exactly those 3 seconds, because if it gets activated immediately after the 3 seconds, then the very end of the 3 seconds might be part of the onset. So we wait a little while (ACTIVITY_ONSET_LEEWAY) before applying the judgment.

*/
const FFT_WINDOW: usize = 50;
const SIZE_OF_CHUNK_OVER_WHICH_MAXIMUM_IS_TAKEN: usize = 100;
const ACTIVITY_ONSET_LEEWAY: usize = 50; // note: the code currently relies on this being less than SIZE_OF_CHUNK_OVER_WHICH_MAXIMUM_IS_TAKEN
const NUMBER_OF_CHUNKS_OVER_WHICH_MAXIMA_ARE_TAKEN: usize = 30;
const FFT_HISTORY_SIZE: usize = 3000;

#[derive(Clone)]
struct ActivityThresholdStats {
    threshold: f64,
    increment: f64,
}
impl Default for ActivityThresholdStats {
    fn default() -> Self {
        ActivityThresholdStats {
            threshold: f64::MAX,
            increment: 0.0000001,
        }
    }
}
#[derive(Default)]
pub struct SingleFrequencyState {
    frequency: f64,

    raw_signal_values: Window<f64, FFT_WINDOW>,
    directions: Window<Complex<f64>, FFT_WINDOW>,
    nudft_summands: Window<Complex<f64>, FFT_WINDOW>,

    corrected_nudft_norms: Window<f64, FFT_HISTORY_SIZE>,
    // for the running stddev:
    // corrected_nudft_norm_squares: Window<f64, FFT_HISTORY_SIZE>,
    chunk_maxima: Window<f64, NUMBER_OF_CHUNKS_OVER_WHICH_MAXIMA_ARE_TAKEN>,
    running_max_of_current_chunk: f64,

    activity_threshold_stats: ActivityThresholdStats,
    activity_threshold_stats_candidate: ActivityThresholdStats,
}

impl SingleFrequencyState {
    pub fn new(frequency: f64) -> SingleFrequencyState {
        SingleFrequencyState {
            frequency,
            ..Default::default()
        }
    }
    pub fn observe_raw_signal_value(
        &mut self,
        raw_signal_value: f64,
        time: f64,
        signal_idle: bool,
    ) {
        let direction = Complex::cis(time * TAU * self.frequency);
        self.raw_signal_values.push(raw_signal_value);
        self.directions.push(direction);
        self.nudft_summands.push(direction * raw_signal_value);

        let corrected_nudft =
            self.nudft_summands.mean() - self.raw_signal_values.mean() * self.directions.mean();
        let corrected_nudft_norm = corrected_nudft.norm();

        self.corrected_nudft_norms.push(corrected_nudft_norm);
        self.running_max_of_current_chunk =
            self.running_max_of_current_chunk.max(corrected_nudft_norm);
        let chunk_phase = self.corrected_nudft_norms.num_values_seen()
            % SIZE_OF_CHUNK_OVER_WHICH_MAXIMUM_IS_TAKEN as u64;
        if chunk_phase == 0 {
            self.chunk_maxima.push(self.running_max_of_current_chunk);
            self.running_max_of_current_chunk = 0.0;
            let mut top_nonadjacent_maxima: ArrayVec<f64, 5> = ArrayVec::new();
            let mut maxima: ArrayVec<f64, NUMBER_OF_CHUNKS_OVER_WHICH_MAXIMA_ARE_TAKEN> =
                self.chunk_maxima.values().copied().collect();
            while !maxima.is_empty() && !top_nonadjacent_maxima.is_full() {
                let (argmax, &max) = maxima
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                    .unwrap();
                top_nonadjacent_maxima.push(max);
                maxima.drain(argmax.saturating_sub(1)..maxima.len().min(argmax + 2));
            }
            let threshold = top_nonadjacent_maxima[0];
            let increment = top_nonadjacent_maxima.into_iter().std_dev().max(0.0000001);
            self.activity_threshold_stats_candidate = ActivityThresholdStats {
                threshold,
                increment,
            }
        }
        if chunk_phase == ACTIVITY_ONSET_LEEWAY as u64 && signal_idle {
            self.activity_threshold_stats = self.activity_threshold_stats_candidate.clone();
        }
        // self.corrected_nudft_norm_squares
        //     .push(corrected_nudft_norm.powi(2));
    }

    // pub fn corrected_nudft_norm_stddev(&self) -> f64 {
    //     return (self.corrected_nudft_norm_squares.mean()
    //         - self.corrected_nudft_norms.mean().powi(2))
    //     .sqrt();
    // }

    pub fn latest_activity_level(&self) -> f64 {
        ((self.corrected_nudft_norms.last().unwrap() - self.activity_threshold_stats.threshold)
            / self.activity_threshold_stats.increment)
            .clamp(0.0, get_variable("max_activity_contribution_per_frequency"))
    }
}

#[derive(Default)]
pub struct Signal {
    pub total_inputs: usize,
    pub recent_raw_inputs: VecDeque<f64>,
    pub history: VecDeque<HistoryFrame>,
    pub frequency_states: Vec<SingleFrequencyState>,
    pub aggregate_activity_level: f64,
    pub frequencies_history: [VecDeque<Vec<f64>>; 3],
    pub active_state: ActiveState,
}

impl Default for ActiveState {
    fn default() -> Self {
        ActiveState::Inactive {
            last_deactivated_time: f64::MIN,
            last_deactivated_sample: i64::MIN,
        }
    }
}
impl Signal {
    // pub fn new() -> Signal {
    //     Signal::default()
    // }
    pub fn is_active(&self) -> bool {
        matches!(self.active_state, ActiveState::Active { .. })
    }
    // pub fn aggregate_activity_level(&self) -> f64 {
    //     self.aggregate_activity_level
    // }
    pub fn receive_raw(
        &mut self,
        raw_value: f64,
        time: f64,
        report_frame: impl FnOnce(HistoryFrame),
        report_frequency_frame: impl FnOnce(FrequenciesFrame),
    ) {
        self.total_inputs += 1;
        self.recent_raw_inputs.push_back(raw_value / 1500.0);
        if self.recent_raw_inputs.len() > 1000 {
            self.recent_raw_inputs.pop_front();
        }

        if self.frequency_states.is_empty() {
            self.frequency_states = (1..FFT_WINDOW)
                .map(|f| f as f64 / (FFT_WINDOW as f64 / 1000.0))
                // .map(|f| 500.0 * 0.895_f64.powi(f as i32))
                .map(|f| SingleFrequencyState::new(f))
                .collect();
        }
        let signal_idle = match self.active_state {
            ActiveState::Inactive {
                last_deactivated_sample,
                ..
            } => {
                last_deactivated_sample
                    + i64::try_from(
                        SIZE_OF_CHUNK_OVER_WHICH_MAXIMUM_IS_TAKEN
                            * NUMBER_OF_CHUNKS_OVER_WHICH_MAXIMA_ARE_TAKEN
                            + ACTIVITY_ONSET_LEEWAY,
                    )
                    .unwrap()
                    < i64::try_from(self.total_inputs).unwrap()
            }
            _ => false,
        };
        for state in &mut self.frequency_states {
            state.observe_raw_signal_value(raw_value / 1500.0, time, signal_idle);
        }
        self.aggregate_activity_level = self
            .frequency_states
            .iter()
            .map(SingleFrequencyState::latest_activity_level)
            .sum::<f64>();
        let values: Vec<f64> = self
            .frequency_states
            .iter()
            .map(|state| state.corrected_nudft_norms.last().unwrap())
            .collect();
        let thresholds: Vec<f64> = self
            .frequency_states
            .iter()
            .map(|state| state.activity_threshold_stats.threshold)
            .collect();

        const FRAMES_PER_FFT: usize = 10;
        if self.recent_raw_inputs.len() >= FFT_WINDOW && self.total_inputs % FRAMES_PER_FFT == 0 {
            // let fft = fft_planner.plan_fft_forward(FFT_WINDOW);
            //
            // let mut buffer: Vec<_> = self
            //     .recent_raw_inputs
            //     .iter()
            //     .rev()
            //     .take(FFT_WINDOW)
            //     .map(|&re| Complex { re, im: 0.0 })
            //     .collect();
            // fft.process(&mut buffer);
            // let values: Vec<f64> = buffer.into_iter().skip(1).map(|c| c.norm()).collect();
            //let scale = 1.0 / values.iter().max_by_key(|&&f| OrderedFloat(f)).unwrap();
            let scale = 50.0;
            // for value in &mut values {
            //     *value *= scale;
            // }
            self.frequencies_history[0].push_back(values.iter().map(|v| v * scale).collect());
            self.frequencies_history[1].push_back(thresholds.iter().map(|v| v * scale).collect());
            let max_activity_contribution_per_frequency =
                get_variable("max_activity_contribution_per_frequency");
            self.frequencies_history[2].push_back(
                self.frequency_states
                    .iter()
                    .map(SingleFrequencyState::latest_activity_level)
                    .map(|a| a / max_activity_contribution_per_frequency)
                    .collect(),
            );
            report_frequency_frame(FrequenciesFrame {
                time,
                values: self
                    .frequencies_history
                    .iter()
                    .map(|h| h.back().unwrap().clone())
                    .collect(),
            });
            for h in &mut self.frequencies_history {
                while h.len() > 800 / FRAMES_PER_FFT {
                    h.pop_front();
                }
            }

            // let fft = fft_planner.plan_fft_forward(VALUE_WINDOW);
            // let mut buffer: Vec<_> = self
            //     .recent_raw_inputs
            //     .iter()
            //     .rev()
            //     .take(VALUE_WINDOW)
            //     .map(|&re| Complex { re, im: 0.0 })
            //     .collect();
            // fft.process(&mut buffer);
            // let values: Vec<f64> = buffer.into_iter().skip(1).map(|c| c.norm_sqr()).collect();
            // let musc = values[4..=5]
            //     .iter()
            //     .chain(&values[7..=8])
            //     .chain(&values[10..15])
            //     .mean();
            // let etc = values[15..35].mean();
            // let value = if musc > etc { (musc - etc).sqrt() } else { 0.0 };

            // let value = self
            //     .recent_raw_inputs
            //     .iter()
            //     .rev()
            //     .take(VALUE_WINDOW)
            //     .std_dev();

            // const CHUNK_SIZE: usize = 500;
            // let (activity_threshold, too_much_threshold) = if self.total_inputs % CHUNK_SIZE == 0 {
            //     let recent_values: Vec<f64> = std::iter::once(value)
            //         .chain(
            //             self.history
            //                 .iter()
            //                 .rev()
            //                 .take_while(|f| f.time >= time - 3.0)
            //                 .map(|f| f.value),
            //         )
            //         .collect();
            //     let max_spike_permitted: f64 = recent_values
            //         .iter()
            //         .copied()
            //         .chunks(CHUNK_SIZE)
            //         .into_iter()
            //         .map(|chunk| {
            //             let sorted: Vec<f64> = chunk.sorted_by_key(|&f| OrderedFloat(f)).collect();
            //             let a = sorted[sorted.len() / 8];
            //             let b = sorted[sorted.len() * 7 / 8];
            //             let d = b - a;
            //             b + d * 2.0
            //         })
            //         .min_by_key(|&f| OrderedFloat(f))
            //         .unwrap();
            //
            //     let is_idle = self
            //         .history
            //         .iter()
            //         .rev()
            //         .take_while(|f| f.time >= time - 3.0)
            //         .all(|f| f.value <= max_spike_permitted);
            //
            //     if is_idle {
            //         let sorted: Vec<f64> = recent_values
            //             .iter()
            //             .copied()
            //             .sorted_by_key(|&f| OrderedFloat(f))
            //             .collect();
            //         let a = sorted[sorted.len() / 8];
            //         let b = sorted[sorted.len() * 7 / 8];
            //         let d = b - a;
            //
            //         (b + d * 1.4, b + d * 15.0)
            //     } else if let Some(back) = self.history.back() {
            //         (back.activity_threshold, back.too_much_threshold)
            //     } else {
            //         (1.0, 1.0)
            //     }
            // } else if let Some(back) = self.history.back() {
            //     (back.activity_threshold, back.too_much_threshold)
            // } else {
            //     (1.0, 1.0)
            // };

            // let recent_values = self
            //     .history
            //     .iter()
            //     .filter(|frame| {
            //         (time - 0.3..time - 0.1).contains(&frame.time) &&
            //             // when we've analyzed something as a spike, also do not count it among the noise
            //             frame.value < frame.activity_threshold
            //     })
            //     .map(|frame| frame.value);
            // let recent_max = recent_values
            //     .max_by_key(|&v| OrderedFloat(v))
            //     .unwrap_or(1.0);

            let value = self.aggregate_activity_level;
            let activity_threshold = get_variable("activity_threshold");
            let too_much_threshold = activity_threshold * 2.0;
            self.history.push_back(HistoryFrame {
                time,
                value: value / activity_threshold,
                activity_threshold: 1.0,
                too_much_threshold: 2.0,
            });
            report_frame(self.history.back().unwrap().clone());
            while self.history.front().unwrap().time < time - 3.0 {
                self.history.pop_front();
            }

            match self.active_state {
                ActiveState::Active {
                    last_sustained_time,
                } => {
                    if value > activity_threshold {
                        self.active_state = ActiveState::Active {
                            last_sustained_time: time,
                        };
                    } else if time > last_sustained_time + 0.1 {
                        self.active_state = ActiveState::Inactive {
                            last_deactivated_time: time,
                            last_deactivated_sample: i64::try_from(self.total_inputs).unwrap(),
                        };
                    }
                }
                ActiveState::Inactive {
                    last_deactivated_time,
                    ..
                } => {
                    if time > last_deactivated_time + 0.4 && value > activity_threshold {
                        self.active_state = ActiveState::Active {
                            last_sustained_time: time,
                        };
                    }
                }
            }
        }
    }
}
