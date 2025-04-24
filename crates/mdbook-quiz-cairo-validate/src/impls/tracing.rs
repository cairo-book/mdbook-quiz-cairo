use regex::Regex;
use std::{
  env::current_dir,
  fs, io,
  path::PathBuf,
  process::{Command, Stdio},
};
use tempfile::TempDir;

use crate::{cxensure, tomlcast, SpannedValue, SpannedValueExt, Validate, ValidationContext};
use mdbook_quiz_schema::*;

use std::path::Path;

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
  fs::create_dir_all(&dst)?;
  for entry in fs::read_dir(src)? {
    let entry = entry?;
    let ty = entry.file_type()?;
    if ty.is_dir() {
      copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
    } else {
      fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
    }
  }
  Ok(())
}

pub fn prepare_crate_for_exercise(file_path: &PathBuf) -> PathBuf {
  // Prepare the crate for the exercise
  let crate_path = current_dir().unwrap().join(PathBuf::from("runner_crate"));
  if !crate_path.exists() {
    panic!("Source runner_crate directory does not exist at: {:?}. This directory is required for exercise validation.", crate_path);
  }
  let src_dir = crate_path.join("src");
  if !src_dir.exists() {
    let _ = fs::create_dir(&src_dir);
  }

  // Create a crate for this exercise only by copying the runner_crate directory and
  // filling the src/lib.cairo file with the content of the file_path
  let dest_crate_path = file_path.parent().unwrap().join("runner_crate");
  copy_dir_all(&crate_path, &dest_crate_path).unwrap();

  let lib_path = dest_crate_path.join("src").join("lib.cairo");
  match fs::copy(file_path, &lib_path) {
      Ok(_) => {},
      Err(err) => panic!("Error occurred while preparing the quiz,\nQuiz: {file_path:?}\nLib path: {lib_path:?}\n{err:?}"),
  };

  dest_crate_path
}

impl Validate for Tracing {
  fn validate(&self, cx: &mut ValidationContext, value: &SpannedValue) {
    let QuestionFields {
      prompt: TracingPrompt { program },
      answer,
      ..
    } = &self.0;
    let mut inner = || -> anyhow::Result<()> {
      let dir = TempDir::new()
        .map_err(|e| anyhow::anyhow!("Failed to create temporary directory: {}", e))?;
      let src_path = dir.path().join("main.cairo");
      fs::write(&src_path, program).map_err(|e| {
        anyhow::anyhow!(
          "Failed to write program to temporary file {}: {}",
          src_path.display(),
          e
        )
      })?;
      let crate_path = prepare_crate_for_exercise(&src_path);

      // Check if crate_path exists
      if !crate_path.exists() {
        anyhow::bail!("Crate path does not exist: {}", crate_path.display());
      }

      let compile_output = Command::new("scarb")
        .arg("build")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(&crate_path)
        .output()
        .map_err(|e| {
          anyhow::anyhow!(
            "Failed to execute scarb build in directory {}: {}",
            crate_path.display(),
            e
          )
        })?;

      let scarb_stderr = String::from_utf8(compile_output.stderr)?;
      let answer_val = tomlcast!(value.table["answer"]);

      if compile_output.status.success() {
        cxensure!(
          cx,
          answer.does_compile,
          labels = vec![tomlcast!(answer_val.table["doesCompile"]).labeled_span()],
          "program compiles but doesCompile = false",
        );

        cxensure!(
          cx,
          answer.stdout.is_some(),
          labels = vec![answer_val.labeled_span()],
          "program compiles but stdout is missing"
        );

        let cmd_output = Command::new("scarb")
          .arg("cairo-run")
          .arg("--no-build")
          .stdout(Stdio::piped())
          .stderr(Stdio::piped())
          .current_dir(crate_path)
          .output()?;

        let cmd_stdout = {
          let output = String::from_utf8(cmd_output.stdout)?;
          let re =
            Regex::new(r"Running runner_crate\s*(.*?)\s*Run completed successfully").unwrap();
          match re.captures(&output) {
            Some(caps) => caps[1].trim().to_string(),
            None => output.trim().to_string(),
          }
        };

        let cmd_stderr = String::from_utf8(cmd_output.stderr)?;

        cxensure!(
          cx,
          cmd_output.status.success(),
          labels = vec![answer_val.labeled_span()],
          "program fails when executed. stderr:\n{}",
          textwrap::indent(&cmd_stderr, "  ")
        );

        let expected_stdout = answer.stdout.as_ref().unwrap();
        cxensure!(
          cx,
          cmd_stdout.trim() == expected_stdout.trim(),
          labels = vec![tomlcast!(answer_val.table["stdout"]).labeled_span()],
          "expected stdout:\n{}\ndid not match actual stdout:\n{}",
          textwrap::indent(expected_stdout, "  "),
          textwrap::indent(&cmd_stdout, "  ")
        );
      } else {
        cxensure!(
          cx,
          !answer.does_compile,
          labels = vec![tomlcast!(answer_val.table["doesCompile"]).labeled_span()],
          "program does not compile but doesCompile = true. scarb stderr:\n{}",
          textwrap::indent(&scarb_stderr, "  ")
        );

        cxensure!(
          cx,
          answer.stdout.is_none(),
          labels = vec![answer_val.labeled_span()],
          "program does not compile but contains a stdout key"
        );
      }

      Ok(())
    };
    inner().unwrap();
  }
}

#[test]
fn validate_tracing_passes() {
  let contents = r#"
[[questions]]
type = "Tracing"
prompt.program = """
fn main() {
  println!("Hello world");
}
"""
answer.doesCompile = true
answer.stdout = "Hello world"
"#;
  assert!(crate::test::harness(contents).is_ok());
}

#[test]
fn cairo_specific_validate_tracing_passes() {
  let contents = r#"
[[questions]]
type = "Tracing"
prompt.program = """
fn main() {
  let mut arr = array![1,2,3];
  let x = arr.pop_front().unwrap();
  println!("{x}");
}
"""
answer.doesCompile = true
answer.stdout = "1"
"#;
  assert!(crate::harness(contents).is_ok());
}

#[test]
fn validate_tracing_compile_fail() {
  let contents = r#"
[[questions]]
type = "Tracing"
prompt.program = """
fn main() {
  let x: String = 1;
}
"""
answer.doesCompile = true
answer.stdout = ""
"#;
  assert!(crate::test::harness(contents).is_err());
}

#[test]
fn validate_tracing_wrong_stdout() {
  let contents = r#"
[[questions]]
type = "Tracing"
prompt.program = """
fn main() {
  println!("Hello world");
}
"""
answer.doesCompile = true
answer.stdout = "meep meep"
"#;
  assert!(crate::test::harness(contents).is_err());
}
