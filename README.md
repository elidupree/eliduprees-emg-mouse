# Eli Dupree's EMG mouse project

My personal project to use EMG sensors for mouse input. Not intended to be an out-of-the-box solution for anyone else, but I do intend to keep this README updated, so that a person with similar programming skills to me would be able to use it as a basis for their own.

## Project structure

`emg-server-c/`: a C program to be run on an ESP32 microcontroller, based on the ESP-IDF gatts_server_service_table and adc_dma examples. It reads input from analog pins and reports the values as BLE notifications. Currently incorrectly advertises itself as a "heart monitor" because I haven't yet learned how to customise that part of the example code. Currently hard-coded to serve to one client at a time, and report every 1 ms. In the future I might make it not waste network power when idle, and maybe do some of the logic.

`emg-client/`: a Rust program to be run on my computers, with several subcommands:
* `emg_client supervisor`: I run this on my Windows computer. It connects to a remote `emg_server`, reads the JSON data, and decides when to emit mouse inputs (currently just clicks). It also serves a GUI web app to localhost and can delegate mouse inputs to other devices (see below). It currently delegates clicks to whichever computer had a mouse-move most recently.
* `emg_client follower`: I run this on my Linux computer. It connects to a remote `emg_client supervisor`, and emits mouse inputs when instructed.

`supervisor/`: Out-of-date (original attempt at the emg-client role, in Python)

`emg-server/`: Out-of-date (original attempt at the EMG server code; a Rust program to be run on an ESP32 microcontroller. Based on [rust-esp32-std-demo](https://github.com/ivmarkov/rust-esp32-std-demo/) (you need to follow the same steps from that repository to build it))

`bluetooth-test/`: simple Rust program for connecting to `emg-server-c`, for my testing
