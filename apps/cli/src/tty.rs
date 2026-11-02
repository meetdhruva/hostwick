//! Reading from the controlling terminal even when stdin is a pipe
//! (e.g. `tar c . | hostwick ssh host 'tar x'` still prompts correctly).

use std::io::{BufRead, Write};

use anyhow::{Result, bail};
use hostwick_core::SecretString;

pub fn read_secret(prompt: &str) -> Result<SecretString> {
    match rpassword::prompt_password(prompt) {
        Ok(s) => Ok(SecretString::from(s)),
        Err(e) => bail!("cannot read from the terminal: {e}"),
    }
}

pub fn read_line(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    #[cfg(unix)]
    let opened = std::fs::File::open("/dev/tty");
    #[cfg(windows)]
    let opened = std::fs::File::open("CONIN$");
    match opened {
        Ok(f) => {
            std::io::BufReader::new(f).read_line(&mut line)?;
        }
        Err(_) => {
            std::io::stdin().lock().read_line(&mut line)?;
        }
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}
