use std::{
    io::{self, BufRead, IsTerminal, Write},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use console::Term;
use dialoguer::{FuzzySelect, theme::ColorfulTheme};
use indicatif::{ProgressBar, ProgressStyle};

pub fn select(title: &str, choices: &[String]) -> Result<usize> {
    ensure!(
        !choices.is_empty(),
        "{title}: no supported selections are available"
    );
    if io::stdin().is_terminal() && Term::stderr().is_term() {
        return FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt(title)
            .items(choices)
            .default(0)
            .max_length(12)
            .interact_on_opt(&Term::stderr())?
            .context("selection cancelled");
    }

    prompt(
        title,
        choices,
        &mut io::stdin().lock(),
        &mut io::stderr().lock(),
    )
}

pub struct Progress(Option<ProgressBar>);

impl Progress {
    pub fn new(interactive: bool, message: String) -> Result<Self> {
        if interactive && Term::stderr().is_term() {
            let bar = ProgressBar::new_spinner();
            bar.set_style(ProgressStyle::with_template(
                "{spinner:.cyan} {msg} [{elapsed_precise}]",
            )?);
            bar.set_message(message);
            bar.enable_steady_tick(Duration::from_millis(100));
            Ok(Self(Some(bar)))
        } else {
            eprintln!("{message}");
            Ok(Self(None))
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if let Some(bar) = &self.0 {
            bar.finish_and_clear();
        }
    }
}

fn prompt(
    title: &str,
    choices: &[String],
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<usize> {
    ensure!(
        !choices.is_empty(),
        "{title}: no supported selections are available"
    );
    writeln!(output, "\n{title}:")?;
    for (index, choice) in choices.iter().enumerate() {
        writeln!(output, "  {}. {choice}", index + 1)?;
    }

    loop {
        write!(output, "Enter a number or name (q to cancel): ")?;
        output.flush()?;
        let mut line = String::new();
        ensure!(
            input.read_line(&mut line)? > 0,
            "selection cancelled: input closed"
        );
        let answer = line.trim();
        ensure!(answer != "q", "selection cancelled");

        if let Some(index) = choices.iter().position(|choice| choice == answer) {
            return Ok(index);
        }
        if let Ok(number) = answer.parse::<usize>()
            && number > 0
            && number <= choices.len()
        {
            return Ok(number - 1);
        }
        writeln!(output, "Choose one of the listed numbers or names.")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_is_retried_and_names_are_accepted() {
        let mut output = Vec::new();
        let selected = prompt(
            "Package",
            &["one".into(), "two".into()],
            &mut &b"0\n3\ntwo\n"[..],
            &mut output,
        )
        .unwrap();
        assert_eq!(selected, 1);
    }

    #[test]
    fn closed_input_cancels_instead_of_looping() {
        assert!(prompt("Package", &["one".into()], &mut &b""[..], &mut Vec::new()).is_err());
    }
}
