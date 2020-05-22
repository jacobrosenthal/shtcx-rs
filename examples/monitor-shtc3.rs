//! Monitor an SHTC3 sensor on Linux in the terminal.

use linux_embedded_hal::{Delay, I2cdev};
use shtcx::{self, Measurement, PowerMode};
use smol::channel::{Receiver, Sender};
use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::time::Duration;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use tui::backend::{Backend, TermionBackend};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Style};
use tui::widgets::{Axis, Block, Borders, Chart, Dataset, Marker, Widget};
use tui::{Frame, Terminal};

const SENSOR_REFRESH_DELAY: Duration = Duration::from_millis(50);
const UI_REFRESH_DELAY: Duration = Duration::from_millis(25);
const DATA_CAPACITY: usize = 100;
const DEVICE: &str = "/dev/i2c-1";

fn main() {
    smol::block_on(async {
        // Handle Ctrl-c
        let ctrl_c = smol::spawn(async move {
            for key in io::stdin().keys() {
                if let Ok(Key::Ctrl('c')) = key {
                    break;
                }
            }
        });

        // Initialize terminal app
        let stdout = io::stdout().into_raw_mode().unwrap();
        let backend = TermionBackend::new(stdout);
        let mut terminal = Terminal::new(backend).unwrap();

        // Prepare terminal
        terminal.clear().unwrap();
        terminal.hide_cursor().unwrap();

        let (sender, receiver) = smol::channel::unbounded();

        //the only thing that CAN return is ctrlc, everthing else loops
        //waiting on https://github.com/stjepang/futures-lite/pull/7
        let _ = futures_micro::or!(ctrl_c, poll(sender), show(receiver, &mut terminal)).await;

        // Reset terminal
        let _ = terminal.clear();
        let _ = terminal.show_cursor();
    });
}

async fn show(
    receiver: Receiver<(Measurement, Measurement)>,
    terminal: &mut Terminal<TermionBackend<RawTerminal<Stdout>>>,
) {
    let mut data = Data::new(DATA_CAPACITY);

    loop {
        // Drain any data updating the buffer
        for (normal, lowpwr) in receiver.try_recv() {
            data.temp_normal
                .push_front(normal.temperature.as_millidegrees_celsius());
            data.temp_lowpwr
                .push_front(lowpwr.temperature.as_millidegrees_celsius());
            data.humi_normal
                .push_front(normal.humidity.as_millipercent());
            data.humi_lowpwr
                .push_front(lowpwr.humidity.as_millipercent());
        }

        data.truncate();
        render(terminal, &data);

        smol::Timer::after(UI_REFRESH_DELAY).await;
    }
}

async fn poll(sender: Sender<(Measurement, Measurement)>) {
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

#[derive(Default)]
struct Data {
    capacity: usize,
    temp_normal: VecDeque<i32>,
    temp_lowpwr: VecDeque<i32>,
    humi_normal: VecDeque<i32>,
    humi_lowpwr: VecDeque<i32>,
}

impl Data {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            ..Default::default()
        }
    }

    /// Truncate data to max `capacity` datapoints.
    fn truncate(&mut self) {
        self.temp_normal.truncate(self.capacity);
        self.temp_lowpwr.truncate(self.capacity);
        self.humi_normal.truncate(self.capacity);
        self.humi_lowpwr.truncate(self.capacity);
    }
}

fn show_chart<B: Backend>(
    title: &str,
    max: (f64, &str),
    data_normal: &[(f64, f64)],
    color_normal: Color,
    data_lowpwr: &[(f64, f64)],
    color_lowpwr: Color,
    frame: &mut Frame<B>,
    area: Rect,
) {
    Chart::default()
        .block(Block::default().title(title).borders(Borders::ALL))
        .x_axis(
            Axis::<&str>::default()
                .title("X Axis")
                .title_style(Style::default().fg(Color::Red))
                .style(Style::default().fg(Color::White))
                .bounds([0.0, DATA_CAPACITY as f64]),
        )
        .y_axis(
            Axis::<&str>::default()
                .title("Y Axis")
                .title_style(Style::default().fg(Color::Red))
                .style(Style::default().fg(Color::White))
                .bounds([0.0, max.0])
                .labels(&["0", max.1]),
        )
        .datasets(&[
            Dataset::default()
                .name("Low power mode")
                .marker(Marker::Braille)
                .style(Style::default().fg(color_lowpwr))
                .data(&data_lowpwr),
            Dataset::default()
                .name("Normal mode")
                .marker(Marker::Dot)
                .style(Style::default().fg(color_normal))
                .data(data_normal),
        ])
        .render(frame, area);
}

fn render(terminal: &mut Terminal<TermionBackend<RawTerminal<Stdout>>>, data: &Data) {
    terminal
        .draw(|mut f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(f.size());
            let (temp_normal, temp_lowpwr, humi_normal, humi_lowpwr) = {
                (
                    data.temp_normal
                        .iter()
                        .rev()
                        .enumerate()
                        .map(|(i, x): (usize, &i32)| (i as f64, (*x as f64) / 1000.0))
                        .collect::<Vec<_>>(),
                    data.temp_lowpwr
                        .iter()
                        .rev()
                        .enumerate()
                        .map(|(i, x): (usize, &i32)| (i as f64, (*x as f64) / 1000.0))
                        .collect::<Vec<_>>(),
                    data.humi_normal
                        .iter()
                        .rev()
                        .enumerate()
                        .map(|(i, x): (usize, &i32)| (i as f64, (*x as f64) / 1000.0))
                        .collect::<Vec<_>>(),
                    data.humi_lowpwr
                        .iter()
                        .rev()
                        .enumerate()
                        .map(|(i, x): (usize, &i32)| (i as f64, (*x as f64) / 1000.0))
                        .collect::<Vec<_>>(),
                )
            };
            show_chart(
                "Temperature",
                (50.0, "50"),
                temp_normal.as_slice(),
                Color::Red,
                temp_lowpwr.as_slice(),
                Color::Magenta,
                &mut f,
                chunks[0],
            );
            show_chart(
                "Humidity",
                (100.0, "100"),
                humi_normal.as_slice(),
                Color::Blue,
                humi_lowpwr.as_slice(),
                Color::Cyan,
                &mut f,
                chunks[1],
            );
        })
        .unwrap();
}
