use anyhow::{bail, Result};
use embedded_hal::adc::OneShot;
use embedded_hal::blocking::delay::DelayUs;
use embedded_svc::wifi::{
    ApStatus, ClientConfiguration, ClientConnectionStatus, ClientIpStatus, ClientStatus,
    Configuration, Status, Wifi,
};
use emg_mouse_shared::{MessageToServer, ReportFromServer, Samples, ServerRunId};
use esp_idf_hal::adc;
use esp_idf_hal::adc::{Atten11dB, PoweredAdc, ADC1};
use esp_idf_hal::delay::{Ets, FreeRtos};
use esp_idf_hal::gpio::{Gpio32, Gpio33, Gpio34, Gpio35};
use esp_idf_hal::prelude::*;
use esp_idf_svc::netif::EspNetifStack;
use esp_idf_svc::nvs::EspDefaultNvs;
use esp_idf_svc::sysloop::EspSysLoopStack;
use esp_idf_svc::wifi::EspWifi;
use log::info;
use rand::random;
use std::cmp::max;
use std::convert::{TryFrom, TryInto};
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const SSID: &str = env!("WIFI_SSID");
const PASS: &str = env!("WIFI_PASS");

fn main() -> Result<()> {
    esp_idf_sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    let netif_stack = Arc::new(EspNetifStack::new()?);
    let sys_loop_stack = Arc::new(EspSysLoopStack::new()?);
    let default_nvs = Arc::new(EspDefaultNvs::new()?);

    #[allow(unused)]
    let wifi = wifi(
        netif_stack.clone(),
        sys_loop_stack.clone(),
        default_nvs.clone(),
    )?;

    let powered_adc1 = adc::PoweredAdc::new(
        peripherals.adc1,
        adc::config::Config::new().calibration(true),
    )?;
    let pin1 = peripherals.pins.gpio32.into_analog_atten_11db()?;
    let pin2 = peripherals.pins.gpio33.into_analog_atten_11db()?;
    let pin3 = peripherals.pins.gpio34.into_analog_atten_11db()?;
    let pin4 = peripherals.pins.gpio35.into_analog_atten_11db()?;

    let (sender, receiver) = mpsc::channel();
    let mut communicator = Communicator::new(receiver);
    let mut sampler = Sampler::new(
        SamplingPeripherals {
            powered_adc1,
            pin1,
            pin2,
            pin3,
            pin4,
        },
        sender,
    );

    // std::thread::Builder::new()
    //     .stack_size(10000)
    //     .spawn(move || {
    //         sampler.take_samples_indefinitely();
    //     })
    //     .unwrap();
    //
    // communicator.run();

    communicator.run_with_sampler_in_same_thread(&mut sampler);
    Ok(())
}

fn wifi(
    netif_stack: Arc<EspNetifStack>,
    sys_loop_stack: Arc<EspSysLoopStack>,
    default_nvs: Arc<EspDefaultNvs>,
) -> Result<Box<EspWifi>> {
    let mut wifi = Box::new(EspWifi::new(netif_stack, sys_loop_stack, default_nvs)?);

    info!("Wifi created, about to scan");

    let ap_infos = wifi.scan()?;

    let ours = ap_infos.into_iter().find(|a| a.ssid == SSID);

    let channel = if let Some(ours) = ours {
        info!(
            "Found configured access point {} on channel {}",
            SSID, ours.channel
        );
        Some(ours.channel)
    } else {
        info!(
            "Configured access point {} not found during scanning, will go with unknown channel",
            SSID
        );
        None
    };

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: SSID.into(),
        password: PASS.into(),
        channel,
        ..Default::default()
    }))?;

    info!("Wifi configuration set, about to get status");

    wifi.wait_status_with_timeout(Duration::from_secs(20), |status| !status.is_transitional())
        .map_err(|e| anyhow::anyhow!("Unexpected Wifi status: {:?}", e))?;

    let status = wifi.get_status();

    if let Status(
        ClientStatus::Started(ClientConnectionStatus::Connected(ClientIpStatus::Done(ip_settings))),
        ApStatus::Stopped,
    ) = status
    {
        info!("Wifi connected with ip settings {:?}", ip_settings);
    } else {
        bail!("Unexpected Wifi status: {:?}", status);
    }

    Ok(wifi)
}

struct SamplingPeripherals {
    powered_adc1: PoweredAdc<ADC1>,
    pin1: Gpio32<Atten11dB<ADC1>>,
    pin2: Gpio33<Atten11dB<ADC1>>,
    pin3: Gpio34<Atten11dB<ADC1>>,
    pin4: Gpio35<Atten11dB<ADC1>>,
}

impl SamplingPeripherals {
    fn read_analog_pins(&mut self) -> [u16; 4] {
        [
            self.powered_adc1.read(&mut self.pin1).unwrap(),
            self.powered_adc1.read(&mut self.pin2).unwrap(),
            self.powered_adc1.read(&mut self.pin3).unwrap(),
            self.powered_adc1.read(&mut self.pin4).unwrap(),
        ]
    }
}

struct Sampler {
    start: Instant,
    num_samples: u64,
    peripherals: SamplingPeripherals,
    sender: Sender<Samples>,
}

impl Sampler {
    fn new(peripherals: SamplingPeripherals, sender: Sender<Samples>) -> Sampler {
        Sampler {
            start: Instant::now(),
            num_samples: 0,
            peripherals,
            sender,
        }
    }

    fn take_sample(&mut self) {
        let sample = Samples {
            time_since_start: Instant::now() - self.start,
            inputs: self.peripherals.read_analog_pins(),
        };
        self.num_samples += 1;
        self.sender.send(sample).unwrap();
    }

    fn wait_for_next_sample(&self) {
        let next_sample_time = self.start + Duration::from_millis(self.num_samples);
        if let Some(delay_needed) = next_sample_time.checked_duration_since(Instant::now()) {
            Ets.delay_us(delay_needed.as_micros() as u32);
            if self.num_samples % 333 == 0 {
                info!(
                    "{:?} late",
                    Instant::now().saturating_duration_since(next_sample_time)
                );
            }
        }
    }

    fn take_samples_indefinitely(&mut self) {
        loop {
            self.take_sample();
            self.wait_for_next_sample();
        }
    }
}

struct ConnectedSupervisor {
    address: SocketAddr,
    last_acknowledged_sample_index: u64,
}

struct Communicator {
    server_run_id: ServerRunId,
    sample_receiver: Receiver<Samples>,
    recent_samples: Vec<Samples>,
    latest_sample_index: u64,
    socket: UdpSocket,
    receive_buffer: [u8; 100],
    current_supervisor: Option<ConnectedSupervisor>,
}

impl Communicator {
    fn new(sample_receiver: Receiver<Samples>) -> Communicator {
        let socket = UdpSocket::bind(concat!("0.0.0.0:", env!("EMG_SERVER_PORT"))).unwrap();
        socket.set_nonblocking(true).unwrap();
        info!("S");
        Communicator {
            server_run_id: random(),
            sample_receiver,
            recent_samples: vec![Samples::default(); 1000],
            socket,
            current_supervisor: None,
            receive_buffer: [0u8; 100],
            latest_sample_index: 0,
        }
    }
    fn store_sample(&mut self, sample: Samples) {
        let index1 = usize::try_from(self.latest_sample_index % 1000).unwrap();
        let index2 = usize::try_from((self.latest_sample_index + 500) % 1000).unwrap();
        if index1 >= self.recent_samples.len() || index2 >= self.recent_samples.len() {
            info!(
                "bounds?? {} {} {}",
                index1,
                index2,
                self.recent_samples.len()
            );
        }
        *self.recent_samples.get_mut(index1).unwrap() = sample;
        self.recent_samples[index2] = sample;
        self.latest_sample_index += 1;
    }
    fn receive_update_if_any(&mut self) {
        if let Ok((size, address)) = self.socket.recv_from(&mut self.receive_buffer) {
            if let Ok(message) =
                bincode::deserialize::<MessageToServer>(&self.receive_buffer[..size])
            {
                self.current_supervisor = Some(ConnectedSupervisor {
                    address,
                    last_acknowledged_sample_index: if message.server_run_id == self.server_run_id {
                        message.latest_received_sample_index
                    } else {
                        0
                    },
                });
            }
        }
    }
    fn send_update(&mut self) {
        if let Some(supervisor) = &self.current_supervisor {
            if let Some(num_unacknowledged_samples) = self
                .latest_sample_index
                .checked_sub(supervisor.last_acknowledged_sample_index)
            {
                if false && num_unacknowledged_samples > 2000 {
                    self.current_supervisor = None;
                } else {
                    let end: usize = max(
                        self.latest_sample_index % 1000,
                        (self.latest_sample_index + 500) % 1000,
                    )
                    .try_into()
                    .unwrap();
                    let samples_to_send = num_unacknowledged_samples
                        .min(self.latest_sample_index)
                        .min(500)
                        .try_into()
                        .unwrap();
                    let unacknowledged_samples =
                        &self.recent_samples[end.saturating_sub(samples_to_send)..end];
                    let buf = bincode::serialize(&ReportFromServer {
                        server_run_id: self.server_run_id,
                        latest_sample_index: self.latest_sample_index,
                        samples: unacknowledged_samples,
                    })
                    .unwrap();
                    let _ = self.socket.send_to(&buf, supervisor.address);
                }
            }
        }
    }
    fn run(&mut self) {
        loop {
            self.store_sample(self.sample_receiver.recv().unwrap());
            while let Ok(sample) = self.sample_receiver.try_recv() {
                self.store_sample(sample);
            }
            self.receive_update_if_any();
            self.send_update();
            FreeRtos.delay_us(300u32);
        }
    }

    fn run_with_sampler_in_same_thread(&mut self, sampler: &mut Sampler) {
        loop {
            sampler.take_sample();
            self.store_sample(self.sample_receiver.recv().unwrap());
            self.receive_update_if_any();
            self.send_update();
            sampler.wait_for_next_sample();
        }
    }
}
