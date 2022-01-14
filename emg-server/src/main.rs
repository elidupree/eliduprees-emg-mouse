use anyhow::{bail, Result};
use embedded_hal::adc::OneShot;
use embedded_hal::blocking::delay::DelayMs;
use embedded_svc::wifi::{
    ApStatus, ClientConfiguration, ClientConnectionStatus, ClientIpStatus, ClientStatus,
    Configuration, Status, Wifi,
};
use esp_idf_hal::adc;
use esp_idf_hal::adc::{Atten11dB, PoweredAdc, ADC1};
use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::Gpio33;
use esp_idf_hal::prelude::*;
use esp_idf_svc::netif::EspNetifStack;
use esp_idf_svc::nvs::EspDefaultNvs;
use esp_idf_svc::sysloop::EspSysLoopStack;
use esp_idf_svc::wifi::EspWifi;
use log::{error, info};
use serde::Serialize;
use std::io::{BufWriter, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

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
    let mut left_button_pin = peripherals.pins.gpio33.into_analog_atten_11db()?;

    let listener = TcpListener::bind(concat!("0.0.0.0:", env!("EMG_SERVER_PORT")))?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                info!("Accepted client");
                let _ = handle_client(stream, &mut powered_adc1, &mut left_button_pin);
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

#[derive(Serialize)]
struct Report {
    left_button: u16,
}

fn handle_client(
    stream: TcpStream,
    powered_adc1: &mut PoweredAdc<ADC1>,
    left_button_pin: &mut Gpio33<Atten11dB<ADC1>>,
) -> Result<()> {
    let mut stream = BufWriter::new(stream);

    loop {
        let left_button = powered_adc1.read(left_button_pin).unwrap();
        serde_json::to_writer(&mut stream, &Report { left_button })?;
        stream.write("\n".as_bytes())?;
        stream.flush()?;
        Ets.delay_ms(5u32);
    }
}
