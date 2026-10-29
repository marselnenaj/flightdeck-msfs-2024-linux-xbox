// SPDX-License-Identifier: MIT
//! Maintainer tooling. Nothing here is part of the installed launcher.
mod build;
mod compat;
mod components;
mod contract;
mod distro;
mod drivers;
#[cfg(test)]
mod entrypoint_tests;
mod http;
mod media;
mod mobiflight;
mod package;
mod package_check;
mod probes;
#[cfg(test)]
mod protocol_tests;
mod release;
mod source;
mod util;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "flightdeck-tools",
    about = "Flightdeck checks, packaging and releases"
)]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect the source-release allowlist; no runtime data is included.
    SourceCheck,
    /// Create an inspected source archive with optional verified build inputs.
    SourceRelease {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        media_sources: Option<PathBuf>,
        #[arg(long)]
        mobiflight_sources: Option<PathBuf>,
    },
    /// Build the release executable with checkout, Cargo and toolchain paths remapped.
    BuildNative {
        #[arg(long)]
        offline: bool,
        /// An installed Rust toolchain, for example 1.98.0.
        #[arg(long)]
        toolchain: Option<String>,
        #[arg(long)]
        bundled_media: bool,
        #[arg(long)]
        bundled_mobiflight: bool,
    },
    /// Run the workspace tests with private process visibility, as in CI.
    NativeTests,
    /// Build the pinned Wine system-memory video patch from its original sources.
    MediaBuild(media::Arguments),
    /// Build the pinned MobiFlight HID compatibility DLL from verified inputs.
    MobiflightBuild(mobiflight::Arguments),
    /// Exercise the native service with synthetic profiles over loopback HTTP.
    HttpContract {
        #[arg(long)]
        binary: PathBuf,
    },
    /// Build deterministic launcher packages from explicit, verified inputs.
    Package {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        cargo_metadata: PathBuf,
        #[arg(long)]
        rust_notices: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        native: Option<PathBuf>,
        #[arg(long)]
        graphics: Option<PathBuf>,
        #[arg(long)]
        launcher_only: bool,
    },
    /// Verify pinned upstream inputs and apply the reviewed compatibility patches.
    CompatStage {
        #[arg(long)]
        winegdk_archive: PathBuf,
        #[arg(long)]
        xodus_repo: PathBuf,
        /// A new directory; the default is build/compat.
        #[arg(long)]
        destination: Option<PathBuf>,
    },
    /// Package built native components with notices and full rebuild sources.
    CompatRelease {
        #[arg(long)]
        stage: PathBuf,
        #[arg(long)]
        vendor: PathBuf,
        #[arg(long)]
        vendor_config: PathBuf,
        #[arg(long)]
        graph: PathBuf,
        #[arg(long)]
        supplements: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, num_args = 1.., required = true)]
        toolchain_notices: Vec<PathBuf>,
    },
    /// Run a packaged launcher on a pinned distribution in an isolated container.
    DistroCheck {
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_parser = distro::DISTRIBUTIONS, required_unless_present = "inside")]
        distribution: Option<String>,
        #[arg(long, hide = true)]
        inside: bool,
    },
    /// Install a package over the previous one, hand the service over and roll back.
    PackageCheck {
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        previous_native: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Optional hardware and Wine checks for developers.
    #[command(subcommand)]
    Probe(probes::Probe),
    /// Prepare pinned release inputs, or publish a verified Actions build.
    #[command(subcommand)]
    Release(Release),
    /// Compatibility component tests against a staged source tree.
    #[command(subcommand)]
    Driver(drivers::Driver),
}

#[derive(Subcommand)]
enum Release {
    /// Report whether this is the exact, unpublished release commit.
    Gate,
    /// Fetch and validate the pinned native, graphics, rollback and source inputs.
    Prepare {
        #[arg(long)]
        cache: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        offline: bool,
    },
    /// Unpack a validated full package into a new directory.
    Unpack {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Write the changelog section of the current version as release notes.
    Notes {
        #[arg(long)]
        output: PathBuf,
    },
    /// Bind the tested assets and corresponding sources to a commit.
    Stage {
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        cache: PathBuf,
        #[arg(long)]
        check: PathBuf,
        #[arg(long)]
        commit: String,
    },
    /// Create a draft, verify the uploaded bytes and publish it as Latest.
    Publish {
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        notes: PathBuf,
        #[arg(long)]
        readback: PathBuf,
    },
}

fn run(command: Command) -> util::Result<u8> {
    // Inside the test container there is no checkout; that command needs none.
    let root = util::root();
    let done: util::Result<()> = match command {
        // Probes and drivers end with a status of their own: 2 is inconclusive or misuse.
        Command::Probe(command) => return probes::run(&root, command),
        Command::Driver(command) => return drivers::run(&root, command),
        Command::SourceCheck => source::check(&root),
        Command::SourceRelease {
            output,
            media_sources,
            mobiflight_sources,
        } => {
            if media_sources.is_some() || mobiflight_sources.is_some() {
                source::release_with_sources(
                    &root,
                    &output,
                    media_sources.as_deref(),
                    mobiflight_sources.as_deref(),
                )
            } else {
                source::release(&root, &output)
            }
        }
        Command::BuildNative {
            offline,
            toolchain,
            bundled_media,
            bundled_mobiflight,
        } => build::native(
            &root,
            offline,
            toolchain.as_deref(),
            bundled_media,
            bundled_mobiflight,
        ),
        Command::MediaBuild(args) => media::build(&root, &args),
        Command::MobiflightBuild(args) => mobiflight::build(&root, &args),
        Command::NativeTests => build::native_tests(&root),
        Command::HttpContract { binary } => contract::check(&binary),
        Command::Package {
            binary,
            cargo_metadata,
            rust_notices,
            output,
            native,
            graphics,
            launcher_only,
        } => package::create(
            &root,
            &package::Inputs {
                binary: &binary,
                cargo_metadata: &cargo_metadata,
                rust_notices: &rust_notices,
                native: native.as_deref(),
                graphics: graphics.as_deref(),
                output: &output,
                launcher_only,
            },
        ),
        Command::CompatStage {
            winegdk_archive,
            xodus_repo,
            destination,
        } => compat::stage(
            &root,
            &winegdk_archive,
            &xodus_repo,
            &destination.unwrap_or_else(|| root.join("build/compat")),
        ),
        Command::CompatRelease {
            stage,
            vendor,
            vendor_config,
            graph,
            supplements,
            output,
            toolchain_notices,
        } => compat::create(
            &root,
            &compat::Release {
                stage: &stage,
                vendor: &vendor,
                vendor_config: &vendor_config,
                graph: &graph,
                supplements: &supplements,
                toolchain_notices: &toolchain_notices,
                output: &output,
            },
        ),
        Command::DistroCheck {
            package,
            output,
            inside: true,
            ..
        } => distro::inside(&package, &output),
        Command::DistroCheck {
            package,
            output,
            distribution,
            ..
        } => distro::outside(
            &mut distro::Daemon,
            &distro::Outside {
                images: &root.join("compat/linux-test-images.json"),
                distribution: distribution.as_deref().unwrap_or_default(),
                package: &package,
                output: &output,
                tool: &distro::tool()?,
                user: (
                    rustix::process::getuid().as_raw(),
                    rustix::process::getgid().as_raw(),
                ),
                container: &distro::container_name(),
            },
        ),
        Command::PackageCheck {
            package,
            previous_native,
            output,
        } => package_check::check(&package, &previous_native, &output),
        Command::Release(command) => {
            let host = release::Actions(&root);
            let pins = release::Pins::current();
            match command {
                Release::Gate => release::gate(&host, &root),
                Release::Prepare {
                    cache,
                    output,
                    offline,
                } => release::prepare(&root, &pins, &cache, &output, offline),
                Release::Unpack { archive, output } => release::unpack(&archive, &output),
                Release::Notes { output } => release::write_notes(&root, &output),
                Release::Stage {
                    assets,
                    cache,
                    check,
                    commit,
                } => release::stage(&root, &pins, &assets, &cache, &check, &commit),
                Release::Publish {
                    assets,
                    notes,
                    readback,
                } => release::publish(&host, &root, &pins, &assets, &notes, &readback),
            }
        }
    };
    done.map(|()| 0)
}

fn main() -> std::process::ExitCode {
    match run(Arguments::parse().command) {
        Ok(code) => code.into(),
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
