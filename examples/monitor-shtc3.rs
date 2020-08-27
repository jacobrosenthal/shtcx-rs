//! Monitor an SHTC3 sensor on Linux in the terminal.

use linux_embedded_hal::{Delay, I2cdev};
use shtcx::{self, Measurement, PowerMode};
use smol::channel::{Receiver, Sender};
use std::time::Duration;

const SENSOR_REFRESH_DELAY: Duration = Duration::from_millis(50);
const UI_REFRESH_DELAY: Duration = Duration::from_millis(25);
const DEVICE: &str = "/dev/i2c-17";

fn main() {
    smol::block_on(async {
        // Handle Ctrl-c

        let (s, ctrl_c) = smol::channel::bounded(10);
        let handle = move || {
            let _ = s.try_send(());
        };
        ctrlc::set_handler(handle).unwrap();

        let (sender, receiver) = smol::channel::unbounded();

        //the only thing that CAN return is ctrlc, everthing else loops
        //waiting on https://github.com/stjepang/futures-lite/pull/7
        let _ = futures_micro::or!(ctrl_c.recv(), poll(sender), show(receiver)).await;
    });
}

async fn show(
    receiver: Receiver<(Measurement, Measurement)>,
) -> Result<(), smol::channel::RecvError> {
    loop {
        // Drain any data updating the buffer
        for (normal, _) in receiver.try_recv() {
            println!("{:?}", normal);
        }
        smol::Timer::after(UI_REFRESH_DELAY).await;
    }
}

async fn poll(sender: Sender<(Measurement, Measurement)>) -> Result<(), smol::channel::RecvError> {
    // Initialize sensor driver
    let dev = I2cdev::new(DEVICE).unwrap();
    let mut sht = shtcx::shtc3(dev);
    let mut delay = Delay;

    loop {
        // Do measurements
        let normal = sht.measure(PowerMode::NormalMode, &mut delay).unwrap();
        let lowpwr = sht.measure(PowerMode::LowPower, &mut delay).unwrap();

        // Send measurements over
        let _ = sender.send((normal, lowpwr)).await;

        smol::Timer::after(SENSOR_REFRESH_DELAY).await;
    }
}
