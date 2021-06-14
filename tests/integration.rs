use assert_cmd::Command;
use assert_cmd::assert::Assert;
use color_eyre::eyre::Result;

#[test]
/// make sure help runs. This indicates the binary works
fn test_help() -> Result<()> {
	let mut cmd: Command = Command::cargo_bin("looper")?;
	let assert: Assert = cmd.arg("---help").assert();
	assert.success().stderr("");
	Ok(())
}

#[test]
/// make sure we have a play command by running `looper play --help`
fn test_play_help() -> Result<()> {
	let mut cmd: Command = Command::cargo_bin("looper")?;
	let assert: Assert = cmd.arg("play").arg("---help").assert();
	assert.success().stderr("");
	Ok(())
}

#[test]
#[ignore]
/// execute the play command, playing a file
fn test_play() {
	let mut cmd: Command = Command::cargo_bin("looper").unwrap();
	let assert: Assert = cmd.arg("play").assert();
	assert.success();
}