/// Creates additional plots in the target/criterion folder for the 'single_node' benchmark
///
/// Reads the criterion output of that benchmark. Obviously, the 'single_node' benchmark needs to be
/// run first.
use std::{cmp::Ordering, fs::File, io::BufReader, path::Path};

use plotly::common::DashType;
use serde::Deserialize;
extern crate plotly;

struct ShadedPlotData<'a> {
    name: &'a str,
    x: Vec<f64>,
    y: Vec<f64>,
    y_lower: Vec<f64>,
    y_upper: Vec<f64>,
}
#[derive(Clone)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
}

fn add_shaded_trace(plot: &mut plotly::Plot, data: ShadedPlotData, color: &Color) {
    let Color { r, g, b } = color;
    let trace_lower = plotly::Scatter::new(data.x.clone(), data.y_lower)
        .show_legend(false)
        .mode(plotly::common::Mode::Lines)
        .line(plotly::common::Line::new().color(plotly::color::Rgba::new(*r, *g, *b, 0.0)));
    plot.add_trace(trace_lower);
    let trace_upper = plotly::Scatter::new(data.x.clone(), data.y_upper)
        .fill(plotly::common::Fill::ToNextY)
        .show_legend(false)
        .mode(plotly::common::Mode::Lines)
        .line(plotly::common::Line::new().color(plotly::color::Rgba::new(*r, *g, *b, 0.0)));
    plot.add_trace(trace_upper);
    let trace_main = plotly::Scatter::new(data.x, data.y)
        .line(plotly::common::Line::new().color(plotly::color::Rgb::new(*r, *g, *b)))
        .name(data.name);
    plot.add_trace(trace_main);
}

#[derive(Deserialize)]
struct Benchmark {
    // group_id: String,
    // value_str: String,
    throughput: BenchmarkThroughput,
}

#[derive(Deserialize)]
struct BenchmarkThroughput {
    #[serde(rename = "Elements")]
    elements: u64,
}

#[derive(Deserialize)]
struct Estimates {
    mean: Estimate,
    // median: Estimate,
    // median_abs_dev: Estimate,
    slope: Option<Estimate>,
    // std_dev: Estimate,
}

#[derive(Deserialize)]
struct Estimate {
    confidence_interval: ConfidenceInterval,
    point_estimate: f64,
    // standard_error: f64,
}

#[derive(Deserialize)]
struct ConfidenceInterval {
    // confidence_level: f64,
    lower_bound: f64,
    upper_bound: f64,
}

// #[derive(Deserialize)]
// struct Samples {
//     // sampling_mode: String,
//     iters: Vec<f64>,
//     times: Vec<f64>,
// }

#[derive(Debug, Clone)]
struct DataPoint {
    time: f64,
    time_lower: f64,
    time_upper: f64,
    num_nets: f64,
    throughput: f64,
    throughput_lower: f64,
    throughput_upper: f64,
}

fn add_line(
    bench_name: &str,
    group_name: &str,
    plot_time: &mut plotly::Plot,
    plot_throughput: &mut plotly::Plot,
    color: Color,
) {
    let manifest_path_str: &'static str = env!("CARGO_MANIFEST_DIR");
    let criterion_path =
        Path::new(manifest_path_str).parent().unwrap().join("target").join("criterion");
    let path = criterion_path.join(bench_name).join(group_name);

    let mut data: Vec<DataPoint> = vec![];
    for entry in path.read_dir().unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() != "report" {
            let p_benchmark = entry.path().join("new").join("benchmark.json");
            println!("Reading benchmark file at {:?}", p_benchmark);
            let file = File::open(p_benchmark).unwrap();
            let reader = BufReader::new(file);
            let benchmark: Benchmark = serde_json::from_reader(reader).unwrap();

            let p_estimates = entry.path().join("new").join("estimates.json");
            println!("Reading estimates file at {:?}", p_estimates);
            let file = File::open(p_estimates).unwrap();
            let reader = BufReader::new(file);
            let estimates: Estimates = serde_json::from_reader(reader).unwrap();

            //// use the slope analysis of criterion directly (or the mean if the slope is not
            //// available)
            let estimate = estimates.slope.unwrap_or(estimates.mean);
            let time = estimate.point_estimate / 1e6; // ns -> ms
            let time_lower = estimate.confidence_interval.lower_bound / 1e6; // ns -> ms;
            let time_upper = estimate.confidence_interval.upper_bound / 1e6; // ns -> ms;

            let num_nets = benchmark.throughput.elements as f64;
            let throughput = num_nets / time * 1e3; // transitions/s (time is in ms)
            let throughput_lower = num_nets / time_upper * 1e3; // transitions/s (time is in ms)
            let throughput_upper = num_nets / time_lower * 1e3; // transitions/s (time is in ms)
            data.push(DataPoint {
                time,
                time_lower,
                time_upper,
                num_nets,
                throughput,
                throughput_lower,
                throughput_upper,
            });
        }
    }
    data.sort_by(|lhs, rhs| {
        if lhs.num_nets == rhs.num_nets {
            Ordering::Equal
        } else if lhs.num_nets < rhs.num_nets {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    });

    let x: Vec<f64> = data.iter().map(|e| e.num_nets).collect();
    let y: Vec<f64> = data.iter().map(|e| e.time).collect();
    let y_lower: Vec<f64> = data.iter().map(|e| e.time_lower).collect();
    let y_upper: Vec<f64> = data.iter().map(|e| e.time_upper).collect();
    let plot_data = ShadedPlotData { name: group_name, x, y, y_lower, y_upper };

    add_shaded_trace(plot_time, plot_data, &color);

    let x: Vec<f64> = data.iter().map(|e| e.num_nets).collect();
    let y: Vec<f64> = data.iter().map(|e| e.throughput).collect();
    let y_lower: Vec<f64> = data.iter().map(|e| e.throughput_lower).collect();
    let y_upper: Vec<f64> = data.iter().map(|e| e.throughput_upper).collect();
    let plot_data = ShadedPlotData { name: group_name, x, y, y_lower, y_upper };

    add_shaded_trace(plot_throughput, plot_data, &color);

    let x0 = data.first().unwrap().num_nets;
    let y0 = data.first().unwrap().throughput;
    let x: Vec<f64> = data.iter().map(|e| e.num_nets).collect();
    let y: Vec<f64> = data.iter().map(|e| e.num_nets / x0 * y0).collect();

    let Color { r, g, b } = color;
    let trace_main = plotly::Scatter::new(x, y)
        .line(
            plotly::common::Line::new()
                .color(plotly::color::Rgb::new(r, g, b))
                .dash(DashType::Dash),
        )
        .name("linear scaling");
    plot_throughput.add_trace(trace_main);
}

fn single_node() {
    let manifest_path_str: &'static str = env!("CARGO_MANIFEST_DIR");
    let criterion_path =
        Path::new(manifest_path_str).parent().unwrap().join("target").join("criterion");

    let mut plot_time = plotly::Plot::new();
    let mut plot_troughput = plotly::Plot::new();

    add_line(
        "single_node",
        "single-node",
        &mut plot_time,
        &mut plot_troughput,
        Color { r: 200, g: 120, b: 0 },
    );

    plot_time.set_layout(
        plotly::Layout::new()
            .x_axis(
                plotly::layout::Axis::new()
                    .type_(plotly::layout::AxisType::Log)
                    .title("# nets")
                    .tick_values([1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0].into())
                    .tick_text(["1", "2", "4", "8", "16", "32", "64", "128"].into()),
            )
            .y_axis(
                plotly::layout::Axis::new()
                    .type_(plotly::layout::AxisType::Log)
                    .title("Time per transition [ms]"),
            ),
    );
    let filename = criterion_path.join("single_node_times");
    let image_format = plotly::ImageFormat::SVG;
    let width = 800;
    let height = 600;
    let scale = 1.0;
    plot_time.write_image(filename, image_format, width, height, scale);

    plot_troughput.set_layout(
        plotly::Layout::new()
            .x_axis(
                plotly::layout::Axis::new()
                    .type_(plotly::layout::AxisType::Log)
                    .title("# nets")
                    .tick_values([1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0].into())
                    .tick_text(["1", "2", "4", "8", "16", "32", "64", "128"].into()),
            )
            .y_axis(
                plotly::layout::Axis::new()
                    .type_(plotly::layout::AxisType::Linear)
                    .title("Throughput [transitions/s]")
                    .range(vec![0, 8000]),
            ),
    );
    let filename = criterion_path.join("single_node_throughput_lin");
    let image_format = plotly::ImageFormat::SVG;
    let width = 800;
    let height = 600;
    let scale = 1.0;
    plot_troughput.write_image(filename, image_format, width, height, scale);

    plot_troughput.set_layout(
        plot_troughput.layout().clone().y_axis(
            plotly::layout::Axis::new()
                .type_(plotly::layout::AxisType::Log)
                .title("Throughput [transitions/s]")
                .range(vec![2.25, 4.1]), // exponents
        ),
    );
    let filename = criterion_path.join("single_node_throughput_log");
    let image_format = plotly::ImageFormat::SVG;
    let width = 800;
    let height = 600;
    let scale = 1.0;
    plot_troughput.write_image(filename, image_format, width, height, scale);
}

fn main() {
    single_node();
}
