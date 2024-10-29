use clap::{Arg, Command};
use regex::Regex;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process;
use text_io::read;

/// Struct to hold scale options and target displays
#[derive(Debug, Clone)]
struct ScaleOptions {
    target_displays: Vec<String>,
    scale_values: Vec<f32>,
}

fn main() -> io::Result<()> {
    // Parse command-line arguments using Clap
    let matches = Command::new("Sway Scale Swapper")
        .version("1.0")
        .author("Your Name <youremail@example.com>")
        .about("Manage scale settings in Sway configuration")
        .arg(
            Arg::new("swap")
                .short('s')
                .long("swap")
                .help("Cycle to the next scale option in ascending order")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    // Determine if the swap flag is present
    let swap = matches.get_flag("swap");

    // Expand the user's home directory and locate the Sway config file
    let config_path = expanduser("~/.config/sway/config").expect("Failed to expand config path");

    // Read all lines from the config file into a vector
    let file = File::open(&config_path).expect("Failed to open config file");
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();

    // Identify the 'Scale Options Start' and 'Scale Options End' indices
    let scale_start = lines
        .iter()
        .position(|line| line.contains("Scale Options Start"))
        .unwrap_or_else(|| {
            eprintln!("Error: 'Scale Options Start' marker not found in the config file.");
            process::exit(1);
        });
    let scale_end = lines
        .iter()
        .position(|line| line.contains("Scale Options End"))
        .unwrap_or_else(|| {
            eprintln!("Error: 'Scale Options End' marker not found in the config file.");
            process::exit(1);
        });

    // Extract the scale options section
    let scale_section = &lines[scale_start..=scale_end];

    // Parse the scale options to get target displays and scale values
    let scale_options = parse_scale_options(scale_section);

    // Determine the current scale by inspecting the output lines
    let current_scale = get_current_scale(&lines, &scale_options.target_displays);

    // Decide on the new scale based on the presence of the swap flag
    let new_scale = if swap {
        Some(get_next_scale(&scale_options.scale_values, current_scale))
    } else {
        prompt_user_for_scale(&scale_options.scale_values, current_scale)?
    };

    // If new_scale is None, the user chose to quit; exit without making changes
    if let Some(scale) = new_scale {
        // Update the scale in the output lines for all target displays
        let updated_lines = update_scale_in_outputs(&lines, &scale_options.target_displays, scale);

        // Write the updated config to a temporary file to ensure atomicity
        let temp_path = Path::new("/home/fribbit/.config/sway/config_temp");
        let temp_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(temp_path)
            .expect("Failed to create temporary config file");
        let mut writer = BufWriter::new(temp_file);

        for line in updated_lines {
            writeln!(writer, "{}", line)?;
        }

        // Rename the temporary file to replace the old configuration
        fs::rename(temp_path, &config_path).expect("Failed to replace the original config file");

        // Reload Sway configuration to apply changes
        if process::Command::new("swaymsg")
            .arg("reload")
            .spawn()
            .is_ok()
        {
            println!("Successfully reloaded Sway configuration.");
        } else {
            eprintln!("Failed to reload Sway configuration.");
        }
    } else {
        println!("No changes made. Exiting.");
    }

    Ok(())
}

/// Function to expand the user's home directory
fn expanduser(path: &str) -> Option<String> {
    if path.starts_with('~') {
        if let Some(home_dir) = dirs::home_dir() {
            let mut expanded = home_dir.to_string_lossy().to_string();
            expanded.push_str(&path[1..]);
            Some(expanded)
        } else {
            None
        }
    } else {
        Some(path.to_string())
    }
}

/// Function to parse the Scale Options section
fn parse_scale_options(lines: &[String]) -> ScaleOptions {
    let mut target_displays = Vec::new();
    let mut scale_values = Vec::new();

    // Regular expressions to extract target displays and scale options
    let target_regex = Regex::new(r"# Target Display = (.+)").unwrap();
    let scale_regex = Regex::new(r"# Scale Options = (.+)").unwrap();

    for line in lines {
        if let Some(captures) = target_regex.captures(line) {
            let display = captures.get(1).unwrap().as_str().trim().to_string();
            target_displays.push(display);
        } else if let Some(captures) = scale_regex.captures(line) {
            let scales_str = captures.get(1).unwrap().as_str();
            scale_values = scales_str
                .split(',')
                .filter_map(|s| s.trim().parse::<f32>().ok())
                .collect();
        }
    }

    // Error handling if no target displays or scale options are found
    if target_displays.is_empty() {
        eprintln!("Error: No target displays found in Scale Options section.");
        process::exit(1);
    }

    if scale_values.is_empty() {
        eprintln!("Error: No scale options found in Scale Options section.");
        process::exit(1);
    }

    ScaleOptions {
        target_displays,
        scale_values,
    }
}

/// Function to determine the current scale by inspecting the output lines for target displays.
fn get_current_scale(lines: &[String], target_displays: &[String]) -> f32 {
    // Regular expression to match uncommented output lines and extract display name and scale
    let output_regex = Regex::new(r#"^output\s+"([^"]+)"\s+scale\s+([0-9.]+)"#).unwrap();

    let mut scales = Vec::new();

    for line in lines {
        if let Some(captures) = output_regex.captures(line) {
            let display = captures.get(1).unwrap().as_str().trim().to_string();
            let scale: f32 = captures
                .get(2)
                .unwrap()
                .as_str()
                .trim()
                .parse()
                .unwrap_or(1.0);

            if target_displays.contains(&display) {
                scales.push(scale);
            }
        }
    }

    if scales.is_empty() {
        eprintln!("Warning: No current scale found for target displays. Defaulting to first scale option.");
        // Default to the first scale option
        1.0
    } else {
        // Ensure all scales are the same; if not, notify the user
        let first_scale = scales[0];
        if scales.iter().all(|&s| (s - first_scale).abs() < 1e-6) {
            first_scale
        } else {
            eprintln!(
                "Warning: Multiple scales found for target displays. Using the first scale: {}",
                first_scale
            );
            first_scale
        }
    }
}

/// Function to get the next scale in ascending order, cycling back to the first if at the end.
fn get_next_scale(scale_values: &[f32], current_scale: f32) -> f32 {
    let mut sorted_scales = scale_values.to_vec();
    sorted_scales.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // Define a small epsilon for floating-point comparison
    let epsilon = 1e-6;

    // Find the index of current_scale in sorted_scales
    let mut index = None;
    for (i, &scale) in sorted_scales.iter().enumerate() {
        if (scale - current_scale).abs() < epsilon {
            index = Some(i);
            break;
        }
    }

    if let Some(i) = index {
        // Move to the next index, wrapping around if necessary
        let next_index = (i + 1) % sorted_scales.len();
        let next_scale = sorted_scales[next_index];
        println!("Swapping scale from {} to {}", current_scale, next_scale);
        next_scale
    } else {
        // If current_scale is not found, default to the first scale
        let first_scale = sorted_scales[0];
        println!(
            "Current scale {} not found in scale options. Using first scale {}",
            current_scale, first_scale
        );
        first_scale
    }
}

/// Function to prompt the user to select a scale from available options, with an option to quit.
fn prompt_user_for_scale(scale_values: &[f32], current_scale: f32) -> io::Result<Option<f32>> {
    println!("Current active scale: {}", current_scale);
    println!("Available scale options:");
    for (i, scale) in scale_values.iter().enumerate() {
        println!("{}. {}", i + 1, scale);
    }
    println!("Q. Quit without making changes");
    println!("Enter the number of the scale you want to apply or 'Q' to quit:");

    loop {
        let input: String = read!();
        let trimmed = input.trim();

        if trimmed.eq_ignore_ascii_case("q") {
            println!("Quitting without making changes.");
            return Ok(None);
        }

        if let Ok(choice) = trimmed.parse::<usize>() {
            if choice > 0 && choice <= scale_values.len() {
                let selected_scale = scale_values[choice - 1];
                println!("Selected scale: {}", selected_scale);
                return Ok(Some(selected_scale));
            }
        }
        println!(
            "Invalid selection. Please enter a number between 1 and {}, or 'Q' to quit.",
            scale_values.len()
        );
    }
}

/// Function to update the scale in the output lines for all target displays
fn update_scale_in_outputs(
    lines: &[String],
    target_displays: &[String],
    new_scale: f32,
) -> Vec<String> {
    // Regular expression to match uncommented output lines and capture parts
    let output_regex = Regex::new(r#"^output\s+"([^"]+)"\s+scale\s+([0-9.]+)"#).unwrap();

    lines
        .iter()
        .map(|line| {
            if let Some(captures) = output_regex.captures(line) {
                let display_name = captures.get(1).unwrap().as_str().trim().to_string();
                // let _current_scale: f32 = captures.get(2).unwrap().as_str().trim().parse().unwrap_or(1.0);

                if target_displays.contains(&display_name) {
                    // Update the scale
                    // Preserve any additional parameters after the scale
                    let rest_start = captures.get(2).unwrap().end();
                    let rest = &line[rest_start..];
                    format!("output \"{}\" scale {}{}", display_name, new_scale, rest)
                } else {
                    // Not a target display; leave the line unchanged
                    line.clone()
                }
            } else {
                // Not an output line; leave it unchanged
                line.clone()
            }
        })
        .collect()
}
