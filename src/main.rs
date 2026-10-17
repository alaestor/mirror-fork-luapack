use anyhow::Result;
use bundle::Bundler;
use clap::Parser;
use full_moon::{parse, visitors::VisitorMut};
use pack::Packer;
use std::fs;
use std::path::PathBuf;

mod bundle;
mod pack;

#[derive(Parser)]
#[command(
    author = "The Unnamed Goose",
    version,
    about = "A basic rust application for efficiently bundling Lua scripts into monolithic releases."
)]
struct Args {
    /// An optional prefix file that will be added to the output
    #[arg(short, long)]
    prefix: Option<PathBuf>,
    /// An optional suffix file that will be added to the output
    #[arg(short, long)]
    suffix: Option<PathBuf>,
    /// The output file to be written
    #[arg(short, long)]
    output: PathBuf,
    /// The input file to be processed
    #[arg(short, long)]
    file: PathBuf,
    #[cfg(feature = "lz4")]
    /// Compress the output using lz4
    #[arg(short, long)]
    lz4: bool,

    /// Use the input file as an entry point for the bundler
    #[arg(short = 'B', long)]
    bundle: bool,
    /// Pack the output, can be used standalone
    #[arg(short = 'P', long)]
    pack: bool,
}

fn main() -> Result<()> {
    let mut args = Args::parse();
    let mut output: Vec<u8> = Vec::new();
    if let Some(prefix) = args.prefix {
        output.extend_from_slice(&fs::read(&prefix)?);
    };

    if !args.bundle && !args.pack {
        args.bundle = true;
        args.pack = true;
    }

    let input = if args.bundle {
        Bundler::bundle(args.file)?
    } else {
        fs::read_to_string(args.file)?
    };

    if args.pack {
        let ast = parse(&input).map_err(|e| anyhow::anyhow!("Parse error: {:?}", e))?;
        output.extend_from_slice(&format!("{}", Packer::new().visit_ast(ast)).as_bytes());
    } else {
        output.extend_from_slice(input.as_bytes());
    }

    #[cfg(feature = "lz4")]
    if args.lz4 {
        output.extend_from_slice(&lz4_flex::compress_prepend_size(&output));
        return Ok(());
    }
    
    if let Some(suffix) = args.suffix {
        output.extend_from_slice(&fs::read(&suffix)?);
    };

    fs::write(&args.output, output)?;
    Ok(())
}
