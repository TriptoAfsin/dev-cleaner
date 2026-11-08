//! Package and build caches for more languages and ecosystems: Java and
//! Kotlin extras, .NET extras, Ruby, Swift, Haskell, Elixir/Erlang, Julia,
//! R, Perl, OCaml, Nim, Crystal, D, Gleam, Elm, Lua, Scala/sbt, Clojure,
//! Groovy, Python tool caches, Corepack, Terraform, Pulumi and Bazel.
//!
//! Windows paths follow each tool's documented default (mostly under
//! %LOCALAPPDATA% or %APPDATA%); macOS and Linux follow the tool's XDG or
//! ~/Library choice. Paths that don't exist are simply not shown.

use crate::caches::{children, How, Risk, Target};
use crate::util;
use std::path::{Path, PathBuf};

fn t(id: &'static str, name: &'static str, eco: &'static str, risk: Risk, about: &'static str, paths: Vec<PathBuf>, how: How) -> Target {
    Target { id, name, eco, about, risk, paths, how, junk: false }
}

/// One path per OS.
fn by_os(win: PathBuf, mac: PathBuf, linux: PathBuf) -> PathBuf {
    if cfg!(windows) {
        win
    } else if cfg!(target_os = "macos") {
        mac
    } else {
        linux
    }
}

pub fn targets(h: &Path) -> Vec<Target> {
    let local = util::local_dir();
    let roaming = util::roaming_dir();
    let cache = util::cache_dir();
    let xdg_cache = h.join(".cache");
    let xdg_data = h.join(".local").join("share");
    let tmp = util::temp_dir();
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();

    vec![
        // ------------------------------------------------ Java, Kotlin, JVM
        t("gradle-wrapper", "Gradle wrapper distributions", "Java", Risk::Slow,
            "Every Gradle version ./gradlew has downloaded, one per project version. The wrapper downloads the one a project needs on its next build. Gradle daemons are stopped first.",
            vec![h.join(".gradle").join("wrapper").join("dists")], How::Delete),
        t("gradle-daemon", "Gradle daemon logs", "Java", Risk::Safe,
            "Logs and registry files of Gradle build daemons. Daemons are stopped first; nothing a build needs lives here.",
            vec![h.join(".gradle").join("daemon")], How::Delete),
        t("maven-wrapper", "Maven wrapper distributions", "Java", Risk::Slow,
            "Maven versions ./mvnw downloaded. Downloaded again on the next build.",
            vec![h.join(".m2").join("wrapper")], How::Delete),
        t("konan", "Kotlin/Native toolchains", "Kotlin", Risk::Slow,
            "Compilers, LLVM and platform libraries Kotlin/Native and Kotlin Multiplatform downloaded (~/.konan). Often several GB; downloaded again by the next native build.",
            vec![h.join(".konan")], How::Delete),
        t("android-cache", "Android build cache", "Android", Risk::Safe,
            "Caches the Android Gradle plugin and SDK tools keep in ~/.android. Your emulators (avd) and signing keys are not touched.",
            vec![h.join(".android").join("cache"), h.join(".android").join("build-cache")], How::Delete),
        t("groovy-grapes", "Groovy Grape cache", "Groovy", Risk::Slow,
            "Dependencies Groovy scripts fetched with @Grab.",
            vec![h.join(".groovy").join("grapes")], How::Delete),
        t("sbt-boot", "sbt launcher cache", "Scala", Risk::Slow,
            "sbt and Scala versions the sbt launcher downloaded. Fetched again by the next sbt run.",
            vec![h.join(".sbt").join("boot")], How::Delete),
        t("clojure-gitlibs", "Clojure git deps", "Clojure", Risk::Slow,
            "Git dependencies the Clojure CLI checked out (~/.gitlibs). Maven deps share ~/.m2.",
            vec![h.join(".gitlibs")], How::Delete),

        // -------------------------------------------------------------- .NET
        t("dotnet-workloads", ".NET workload leftovers", ".NET", Risk::Safe,
            "Workload packs left behind by SDK updates. dotnet workload clean removes only packs no installed SDK uses.",
            vec![], How::Command { tool: "dotnet", args: &["workload", "clean"] }),

        // -------------------------------------------------------------- Ruby
        t("ruby-gems", "RubyGems package cache", "Ruby", Risk::Safe,
            "The .gem files kept after installing gems. Installed gems keep working; gem install fetches packages again when needed.",
            {
                let mut v = children(&h.join(".gem").join("ruby"), |_| true, &["cache"]);
                v.extend(children(&xdg_data.join("gem").join("ruby"), |_| true, &["cache"]));
                v
            }, How::Delete),
        t("bundler-cache", "Bundler cache", "Ruby", Risk::Safe,
            "Gems Bundler downloaded into its global cache.",
            vec![h.join(".bundle").join("cache")], How::Delete),
        t("ruby-builds", "Ruby version manager downloads", "Ruby", Risk::Safe,
            "Source archives rbenv, rvm and asdf downloaded to build Ruby versions. Installed Rubies are kept.",
            vec![h.join(".rbenv").join("cache"), h.join(".rvm").join("archives"), h.join(".rvm").join("src")], How::Delete),

        // ------------------------------------------------------------- Swift
        t("swiftpm", "Swift Package Manager cache", "Swift", Risk::Slow,
            "Package repositories and manifests SwiftPM and Xcode cached. Fetched again on the next resolve.",
            vec![
                by_os(local.join("org.swift.swiftpm"), cache.join("org.swift.swiftpm"), xdg_cache.join("org.swift.swiftpm")),
                h.join("Library").join("org.swift.swiftpm"),
            ], How::Delete),

        // ----------------------------------------------------------- Haskell
        t("cabal", "Cabal package cache", "Haskell", Risk::Slow,
            "Package tarballs and index Cabal downloaded. Fetched again by cabal update and the next build.",
            vec![by_os(roaming.join("cabal").join("packages"), h.join(".cabal").join("packages"), xdg_cache.join("cabal").join("packages")), h.join(".cabal").join("packages")],
            How::Delete),
        t("stack", "Stack Pantry cache", "Haskell", Risk::Slow,
            "Package sources Stack's Pantry downloaded. Installed GHC versions are kept.",
            vec![by_os(roaming.join("stack").join("pantry"), h.join(".stack").join("pantry"), h.join(".stack").join("pantry"))], How::Delete),
        t("ghcup-cache", "GHCup downloads", "Haskell", Risk::Safe,
            "Installer archives GHCup kept after installing GHC, Cabal and HLS.",
            vec![h.join(".ghcup").join("cache"), PathBuf::from(r"C:\ghcup\cache")], How::Delete),

        // ---------------------------------------------------- Elixir, Erlang
        t("hex", "Hex package cache", "Elixir", Risk::Slow,
            "Elixir and Erlang packages Hex downloaded. Your Hex credentials in ~/.hex are kept.",
            vec![h.join(".hex").join("packages")], How::Delete),
        t("rebar3", "rebar3 cache", "Erlang", Risk::Slow,
            "Packages and plugins rebar3 cached.",
            vec![by_os(local.join("rebar3"), xdg_cache.join("rebar3"), xdg_cache.join("rebar3"))], How::Delete),

        // ---------------------------------------------------- Julia, R, etc.
        t("julia-compiled", "Julia precompile cache", "Julia", Risk::Safe,
            "Precompiled package images. Julia recompiles them the next time packages load; the first load is slower.",
            vec![h.join(".julia").join("compiled")], How::Delete),
        t("renv", "R renv cache", "R", Risk::Slow,
            "Packages renv caches and links into R projects. Projects using them reinstall with renv::restore().",
            vec![by_os(local.join("R").join("cache").join("R").join("renv"), cache.join("org.R-project.R").join("R").join("renv"), xdg_cache.join("R").join("renv"))],
            How::Delete),
        t("cpanm", "Perl cpanm work files", "Perl", Risk::Safe,
            "Build directories and logs cpanm and CPAN leave behind. Installed modules are kept.",
            vec![h.join(".cpanm").join("work"), h.join(".cpan").join("build"), h.join(".cpan").join("sources")], How::Delete),
        t("opam-cache", "opam download cache", "OCaml", Risk::Safe,
            "Source archives opam downloaded. Installed switches are kept.",
            vec![h.join(".opam").join("download-cache")], How::Delete),
        t("nimble", "Nimble packages", "Nim", Risk::Slow,
            "Nim packages Nimble installed globally. Projects reinstall them with nimble install.",
            vec![h.join(".nimble").join("pkgs2"), h.join(".nimble").join("pkgs")], How::Delete),
        t("crystal-cache", "Crystal compiler cache", "Crystal", Risk::Safe,
            "Object files the Crystal compiler caches between builds.",
            vec![by_os(local.join("crystal"), xdg_cache.join("crystal"), xdg_cache.join("crystal"))], How::Delete),
        t("dub", "D (dub) packages", "D", Risk::Slow,
            "D packages dub fetched. Fetched again on the next build.",
            vec![by_os(local.join("dub").join("packages"), h.join(".dub").join("packages"), h.join(".dub").join("packages"))], How::Delete),
        t("gleam-cache", "Gleam package cache", "Gleam", Risk::Slow,
            "Packages Gleam downloaded.",
            vec![by_os(local.join("gleam"), cache.join("gleam"), xdg_cache.join("gleam"))], How::Delete),
        t("elm", "Elm package cache", "Elm", Risk::Slow,
            "Elm packages and compiler artifacts. Downloaded again by elm make.",
            vec![by_os(roaming.join("elm"), h.join(".elm"), h.join(".elm"))], How::Delete),
        t("luarocks", "LuaRocks cache", "Lua", Risk::Safe,
            "Rock files LuaRocks downloaded.",
            vec![xdg_cache.join("luarocks"), roaming.join("luarocks").join("cache")], How::Delete),

        // ---------------------------------------------------- Python, Node
        t("python-tools", "pdm, Hatch & pipx caches", "Python", Risk::Safe,
            "Download caches of pdm, Hatch and pipx. Environments and installed apps are kept.",
            vec![
                by_os(local.join("pdm").join("Cache"), cache.join("pdm"), xdg_cache.join("pdm")),
                by_os(local.join("hatch").join("Cache"), cache.join("hatch"), xdg_cache.join("hatch")),
                h.join(".local").join("pipx").join(".cache"),
                h.join("pipx").join(".cache"),
            ], How::Delete),
        t("pyenv-cache", "pyenv downloads", "Python", Risk::Safe,
            "Python source archives pyenv kept after building versions. Installed Pythons are kept.",
            vec![h.join(".pyenv").join("cache"), h.join(".pyenv").join("pyenv-win").join("install_cache")], How::Delete),
        t("corepack", "Corepack cache", "Node", Risk::Safe,
            "pnpm and Yarn versions Corepack downloaded for projects that pin them. Fetched again on demand.",
            vec![by_os(local.join("node").join("corepack"), xdg_cache.join("node").join("corepack"), xdg_cache.join("node").join("corepack"))],
            How::Delete),

        // ------------------------------------------------- Infrastructure
        t("terraform", "Terraform plugin cache", "Terraform", Risk::Slow,
            "Provider plugins Terraform cached. terraform init downloads them again.",
            vec![h.join(".terraform.d").join("plugin-cache"), roaming.join("terraform.d").join("plugin-cache")], How::Delete),
        t("pulumi", "Pulumi plugins", "Pulumi", Risk::Slow,
            "Provider and language plugins Pulumi downloaded. Installed again on the next pulumi up.",
            vec![h.join(".pulumi").join("plugins")], How::Delete),
        t("bazel", "Bazel output and caches", "Bazel", Risk::Slow,
            "Bazel's output base for every workspace plus its repository cache. Stop Bazel servers first (bazel shutdown); the next build starts from scratch.",
            vec![
                by_os(h.join(format!("_bazel_{user}")), cache.join("bazel"), xdg_cache.join("bazel")),
                PathBuf::from(format!("/private/var/tmp/_bazel_{user}")),
            ]
            .into_iter()
            .filter(|p| p.as_os_str().len() > 1)
            .collect(), How::Delete),
        t("nuget-scratch", "NuGet temp", ".NET", Risk::Safe,
            "Scratch files NuGet leaves in the temp folder during restores.",
            vec![tmp.join("NuGetScratch")], How::Delete),
    ]
}
