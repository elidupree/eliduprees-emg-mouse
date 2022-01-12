# Eli Dupree's EMG mouse project

My personal project to use EMG sensors for mouse input. Not intended to be an out-of-the-box solution for anyone else, but I do intend to keep this README updated, so that a person with similar programming skills to me would be able to use it as a basis for their own.

## Project structure (none of these parts are completed yet)

`emg-server/`: a Rust program to be run on an ESP32 microcontroller. It reads input from analog pins and serves it as JSON on a local Wi-Fi network. Based on [rust-esp32-std-demo](https://github.com/ivmarkov/rust-esp32-std-demo/) (you need to follow the same steps from that repository to build it).

`supervisor/`: a Python program to be run on my Windows computer (although the program is intended to be cross-platform). It connects to `emg-server`, reads the JSON data, and decides when to emit clicks (using PyAutoGUI).
