use crate::webserver::HistoryFrame;
use ordered_float::OrderedFloat;
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use std::collections::VecDeque;
use std::time::Duration;

// struct Signals {
//     signals: Vec<Signal>,
// }
#[derive(Default)]
pub struct Signal {
    pub total_inputs: usize,
    pub recent: VecDeque<f64>,
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
        self.recent.push_back(raw_value / 1500.0);
        if self.recent.len() > 50 {
            self.recent.pop_front();
        }

        if self.recent.len() == 50 && self.total_inputs % 10 == 0 {
            let fft = fft_planner.plan_fft_forward(50);

            let mut buffer: Vec<_> = self
                .recent
                .iter()
                .map(|&re| Complex { re, im: 0.0 })
                .collect();
            fft.process(&mut buffer);
            self.frequencies_history
                .push_back(buffer.into_iter().map(|c| c.re).collect());
            if self.frequencies_history.len() > 80 {
                self.frequencies_history.pop_front();
            }
        }

        let mean = self.recent.iter().sum::<f64>() / self.recent.len() as f64;
        let variance =
            self.recent.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / self.recent.len() as f64;

        let value = variance.sqrt(); //report.inputs[2] as f64 / 1000.0;
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
            click_threshold: recent_max + 0.005,
            too_much_threshold: recent_max + 0.06,
        });
        self.history.retain(|frame| frame.time >= time - 0.8);
    }
}
