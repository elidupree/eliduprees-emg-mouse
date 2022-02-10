use crate::webserver::HistoryFrame;
use ordered_float::OrderedFloat;
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use statrs::statistics::Statistics;
use std::collections::VecDeque;
use std::time::Duration;

// struct Signals {
//     signals: Vec<Signal>,
// }
#[derive(Default)]
pub struct Signal {
    pub total_inputs: usize,
    pub recent_raw_inputs: VecDeque<f64>,
    pub history: VecDeque<HistoryFrame>,
    pub frequencies_history: VecDeque<Vec<f64>>,
}

impl Signal {
    pub fn new() -> Signal {
        Signal::default()
    }
    pub fn receive_raw(
        &mut self,
        raw_value: f64,
        remote_time_since_start: Duration,
        fft_planner: &mut FftPlanner<f64>,
    ) {
        self.total_inputs += 1;
        self.recent_raw_inputs.push_back(raw_value / 1500.0);
        if self.recent_raw_inputs.len() > 1000 {
            self.recent_raw_inputs.pop_front();
        }

        const FFT_WINDOW: usize = 50;
        const FRAMES_PER_FFT: usize = 10;
        if self.recent_raw_inputs.len() >= FFT_WINDOW && self.total_inputs % FRAMES_PER_FFT == 0 {
            let fft = fft_planner.plan_fft_forward(FFT_WINDOW);

            let mut buffer: Vec<_> = self
                .recent_raw_inputs
                .iter()
                .rev()
                .take(FFT_WINDOW)
                .map(|&re| Complex { re, im: 0.0 })
                .collect();
            fft.process(&mut buffer);
            let values: Vec<f64> = buffer.into_iter().skip(1).map(|c| c.norm()).collect();
            // let scale = 1.0 / values.iter().max_by_key(|&&f| OrderedFloat(f)).unwrap();
            // for value in &mut values {
            //     *value *= scale;
            // }
            self.frequencies_history.push_back(values);
            if self.frequencies_history.len() > 800 / FRAMES_PER_FFT {
                self.frequencies_history.pop_front();
            }
        }

        const VALUE_WINDOW: usize = 50;
        if self.recent_raw_inputs.len() > VALUE_WINDOW {
            let fft = fft_planner.plan_fft_forward(VALUE_WINDOW);
            let mut buffer: Vec<_> = self
                .recent_raw_inputs
                .iter()
                .rev()
                .take(VALUE_WINDOW)
                .map(|&re| Complex { re, im: 0.0 })
                .collect();
            fft.process(&mut buffer);
            let values: Vec<f64> = buffer.into_iter().skip(1).map(|c| c.norm_sqr()).collect();
            let musc = values[4..=5]
                .iter()
                .chain(&values[7..=8])
                .chain(&values[10..15])
                .mean();
            let etc = values[15..35].mean();
            let value = if musc > etc { (musc - etc).sqrt() } else { 0.0 };

            // let value = self
            //     .recent_raw_inputs
            //     .iter()
            //     .rev()
            //     .take(VALUE_WINDOW)
            //     .std_dev();

            let time = remote_time_since_start.as_secs_f64();
            let recent_values = self
                .history
                .iter()
                .filter(|frame| {
                    (time - 0.3..time - 0.1).contains(&frame.time) &&
                        // when we've analyzed something as a spike, also do not count it among the noise
                        frame.value < frame.click_threshold
                })
                .map(|frame| frame.value);
            let recent_max = recent_values
                .max_by_key(|&v| OrderedFloat(v))
                .unwrap_or(1.0);

            self.history.push_back(HistoryFrame {
                time,
                value,
                click_threshold: recent_max + 0.01,
                too_much_threshold: recent_max + 0.25,
            });
            self.history.retain(|frame| frame.time >= time - 0.8);
        }
    }
}
