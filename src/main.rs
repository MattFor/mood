use std::io::IsTerminal;

use chrono::Local;

use mood::cli::{self, Command};
use mood::data;
use mood::plot;
use mood::tui;

fn main() {
	if let Err(err) = run() {
		eprintln!("mood: {err}");
		std::process::exit(1);
	}
}

fn run() -> Result<(), String> {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let now = Local::now();
	match cli::parse(&args, now)? {
		Command::Help => {
			println!("{}", cli::help());
			Ok(())
		}
		Command::Version => {
			println!("mood {}", env!("CARGO_PKG_VERSION"));
			Ok(())
		}
		Command::Status => {
			data::ensure()?;
			let entries = data::load()?;
			let _ = data::write_summary(&entries);
			println!("{}", data::status_line(&entries));
			Ok(())
		}
		Command::Path => {
			println!("{}", data::host_log_path().display());
			Ok(())
		}
		Command::Doctor => doctor(),
		Command::Undo => {
			println!("{}", data::undo()?);
			Ok(())
		}
		Command::Log { mood, reasons, at } => {
			println!("{}", data::log_mood(mood, reasons, at)?);
			Ok(())
		}
		Command::Reasons { span } => {
			data::ensure()?;
			println!("{}", data::reasons_report(&data::load()?, span));
			Ok(())
		}
		Command::Stats { span, bucket, kind } => {
			data::ensure()?;
			let entries = data::load()?;
			let _ = data::write_summary(&entries);
			tui::run(entries, tui::Ui::new(span, bucket, kind))
		}
		Command::Graph {
			span,
			bucket,
			kind,
			out,
		} => {
			data::ensure()?;
			let entries = data::load()?;
			let _ = data::write_summary(&entries);
			// Arguments or a pipe skip the questions, for scripts.
			let (span, bucket, kind, path) = if args.len() == 1 && std::io::stdin().is_terminal() {
				cli::ask_graph(
					&mut std::io::stdin().lock(),
					&mut std::io::stdout(),
					now.date_naive(),
					bucket,
					kind,
				)?
			} else {
				(span, bucket, kind, plot::output_path(out, span, bucket, kind))
			};
			plot::render(&entries, span, bucket, kind, path.clone())?;
			println!("{}", path.display());
			Ok(())
		}
	}
}

fn doctor() -> Result<(), String> {
	data::ensure()?;
	let entries = data::load()?;
	let _ = data::write_summary(&entries);
	let python = plot::python_cmd();
	println!("data     {}", data::data_dir().display());
	println!("log      {}", data::host_log_path().display());
	println!("logs     {}", entries.len());
	println!("python   {python}");
	if plot::matplotlib_ok(&python) {
		println!("plot     matplotlib ok");
		Ok(())
	} else {
		Err(plot::missing_matplotlib(&python))
	}
}
