use anyhow::{bail, Result};
use embedded_hal::adc::OneShot;
use embedded_hal::blocking::delay::DelayUs;
use embedded_svc::wifi::{
    ApStatus, ClientConfiguration, ClientConnectionStatus, ClientIpStatus, ClientStatus,
    Configuration, Status, Wifi,
};
use emg_mouse_shared::{ReportFromServer, HEARTBEAT_DURATION};
use esp_idf_hal::adc;
use esp_idf_hal::adc::{Atten11dB, PoweredAdc, ADC1};
use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{Gpio32, Gpio33, Gpio34, Gpio35};
use esp_idf_hal::prelude::*;
use esp_idf_svc::netif::EspNetifStack;
use esp_idf_svc::nvs::EspDefaultNvs;
use esp_idf_svc::sysloop::EspSysLoopStack;
use esp_idf_svc::wifi::EspWifi;
use log::{error, info};
use std::io::{BufWriter, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
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

    let mut powered_adc1 = adc::PoweredAdc::new(
        peripherals.adc1,
        adc::config::Config::new().calibration(true),
    )?;
    let mut pin1 = peripherals.pins.gpio32.into_analog_atten_11db()?;
    let mut pin2 = peripherals.pins.gpio33.into_analog_atten_11db()?;
    let mut pin3 = peripherals.pins.gpio34.into_analog_atten_11db()?;
    let mut pin4 = peripherals.pins.gpio35.into_analog_atten_11db()?;

    let listener = TcpListener::bind(concat!("0.0.0.0:", env!("EMG_SERVER_PORT")))?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                info!("Accepted client");
                let _ = handle_client(
                    stream,
                    &mut powered_adc1,
                    &mut pin1,
                    &mut pin2,
                    &mut pin3,
                    &mut pin4,
                );
            }
            Err(e) => {
                error!("Error: {}", e);
            }
        }
    }

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

fn handle_client(
    stream: TcpStream,
    powered_adc1: &mut PoweredAdc<ADC1>,
    pin1: &mut Gpio32<Atten11dB<ADC1>>,
    pin2: &mut Gpio33<Atten11dB<ADC1>>,
    pin3: &mut Gpio34<Atten11dB<ADC1>>,
    pin4: &mut Gpio35<Atten11dB<ADC1>>,
) -> Result<()> {
    let mut stream = BufWriter::new(stream);
    let start = Instant::now();
    let mut next_report_time = start;
    const MAX_CATCH_UP_DURATION: Duration = Duration::from_millis(2);
    let mut previous_report = ReportFromServer {
        time_since_start: Duration::from_secs(0),
        inputs: [0; 4],
    };

    loop {
        let report = ReportFromServer {
            time_since_start: Instant::now() - start,
            inputs: [
                powered_adc1.read(pin1).unwrap(),
                powered_adc1.read(pin2).unwrap(),
                powered_adc1.read(pin3).unwrap(),
                powered_adc1.read(pin4).unwrap(),
            ],
        };
        if true
            || report.inputs != previous_report.inputs
            || report.time_since_start > previous_report.time_since_start + HEARTBEAT_DURATION
        {
            bincode::serialize_into(&mut stream, &report)?;
            stream.flush()?;
            previous_report = report;
            //stream.write("\n".as_bytes())?;
            let now = Instant::now();
            if now > next_report_time + MAX_CATCH_UP_DURATION {
                next_report_time = now;
            }
            next_report_time += Duration::from_millis(1);
            if let Some(delay_needed) = next_report_time.checked_duration_since(now) {
                Ets.delay_us(delay_needed.as_micros() as u32);
            }
        }
    }
}
