{
  # Concat on Nix, Linux only: the people asking for a flake run Linux.
  #
  #   nix build            the editor window, ./result/bin/concat
  #   nix run              build and launch it
  #   nix develop          a shell with everything `cargo run -p concat` needs
  #
  # The build is one Cargo workspace under src/, so cargo dependencies
  # come straight from src/Cargo.lock and there is no vendor hash to keep
  # in sync. Three native pieces need care inside the sandbox, which has no
  # network:
  #
  # - FFmpeg is linked, not spawned. nixpkgs' ffmpeg_8 provides the headers
  #   and libraries; bindgen finds them through pkg-config and bindgenHook's
  #   libclang.
  # - whisper.cpp is compiled in by cmake from the source vendored inside
  #   whisper-rs-sys, so it needs cmake and a C++ toolchain and nothing else.
  # - sherpa-onnx (text to speech) links prebuilt static libraries that its
  #   sys crate would download at build time. They are fetched here as fixed-
  #   output derivations instead and handed over with SHERPA_ONNX_ARCHIVE_DIR.
  # - ONNX Runtime (the cutout models) is likewise downloaded by its sys
  #   crate. nixpkgs' onnxruntime is linked instead, through ORT_LIB_LOCATION,
  #   dynamically, so the wrapper's rpath finds it.
  # - Skia, the window's renderer as on every other platform, links prebuilt
  #   binaries that skia-bindings would download at build time. They are
  #   fetched here too and handed over as a file:// SKIA_BINARIES_URL; with
  #   a stale key or version the build falls back to compiling Skia from
  #   source, which the sandbox cannot do, so a mismatch fails loudly.
  #
  # Vulkan is loaded at run time, hence the LD_LIBRARY_PATH on the wrapper.
  description = "Concat - free and open source video editor";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      eachSystem = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # Must match the sherpa-onnx-sys version in src/Cargo.lock: the sys
      # crate names the archive after its own version and refuses any other.
      sherpaVersion = "1.13.7";
      sherpaArchives = {
        x86_64-linux = {
          name = "sherpa-onnx-v${sherpaVersion}-linux-x64-static-lib.tar.bz2";
          hash = "sha256-0b56aawrMBIAWNgwLmJCOaMGQIU4PPpHmUoU/cRMMtY=";
        };
        aarch64-linux = {
          name = "sherpa-onnx-v${sherpaVersion}-linux-aarch64-static-lib.tar.bz2";
          hash = "sha256-dDtN66urLp9lTjk9pyY72Xlaq+1groDIMCMFQINBXRQ=";
        };
      };
      sherpaArchiveDir =
        pkgs:
        let
          archive = sherpaArchives.${pkgs.stdenv.hostPlatform.system};
        in
        pkgs.linkFarm "sherpa-onnx-archives" [
          {
            name = archive.name;
            path = pkgs.fetchurl {
              url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v${sherpaVersion}/${archive.name}";
              inherit (archive) hash;
            };
          }
        ];

      # Must match skia-bindings in src/Cargo.lock: the archive is named
      # after the crate version (the tag) and a key of rust-skia's commit,
      # the target and the Skia features i-slint-renderer-skia turns on for
      # Linux (gl, vulkan, plus skia-safe's defaults). A Slint or skia-safe
      # bump changes these; the new name is printed by skia-bindings' build
      # script as "TRYING TO DOWNLOAD AND INSTALL SKIA BINARIES: tag/key".
      skiaVersion = "0.153.3";
      skiaKeyFeatures = "ganesh-gl-jpegd-jpege-pdf-vulkan";
      skiaArchives = {
        x86_64-linux = {
          key = "b7f043e0b1e2a850e702-x86_64-unknown-linux-gnu-${skiaKeyFeatures}";
          hash = "sha256-nr5MRIzJ94m60kHL0Rc5zfdZ4n21cZwjp2NeCW1YB7M=";
        };
        aarch64-linux = {
          key = "b7f043e0b1e2a850e702-aarch64-unknown-linux-gnu-${skiaKeyFeatures}";
          hash = "sha256-gtoiVv0M6UGVJ/cspOmFihPBTSMHPaHmGNxZY5f2VWA=";
        };
      };
      # The URL template skia-bindings expands; {tag} and {key} are its own
      # placeholders, not Nix's.
      skiaBinariesUrl =
        pkgs:
        let
          archive = skiaArchives.${pkgs.stdenv.hostPlatform.system};
          name = "skia-binaries-${archive.key}.tar.gz";
          dir = pkgs.linkFarm "skia-binaries" [
            {
              inherit name;
              path = pkgs.fetchurl {
                url = "https://github.com/rust-skia/skia-binaries/releases/download/${skiaVersion}/${name}";
                inherit (archive) hash;
              };
            }
          ];
        in
        "file://${dir}/skia-binaries-{key}.tar.gz";

      # Libraries the binary opens at run time rather than links: the Vulkan
      # loader for wgpu, and the windowing libraries winit dlopens.
      runtimeLibs =
        pkgs: with pkgs; [
          vulkan-loader
          libGL
          wayland
          libxkbcommon
          libx11
          libxcursor
          libxi
          libxrandr
        ];

      nativeInputs =
        pkgs: with pkgs; [
          pkg-config
          cmake
          rustPlatform.bindgenHook
        ];

      buildInputs =
        pkgs: with pkgs; [
          # 8, by name: the engine needs 7 or newer, and nixpkgs' unversioned
          # `ffmpeg` is whichever major the distribution defaults to.
          ffmpeg_8
          # The cutout models' runtime; see ORT_LIB_LOCATION above.
          onnxruntime
          alsa-lib
          fontconfig
          freetype
          gtk3
          libxkbcommon
          wayland
          openssl
        ];
    in
    {
      packages = eachSystem (pkgs: rec {
        default = concat;
        concat = pkgs.rustPlatform.buildRustPackage {
          pname = "concat";
          version = "0.2.8";
          src = self;

          cargoRoot = "src";
          buildAndTestSubdir = "src";
          cargoLock.lockFile = ./src/Cargo.lock;
          cargoBuildFlags = [
            "-p"
            "concat"
          ];

          # The workspace's tests generate their own media through the
          # linked encoder and run anywhere the engine builds, but they take
          # minutes; `cargo test` in `nix develop` is where they belong.
          doCheck = false;

          env.SHERPA_ONNX_ARCHIVE_DIR = sherpaArchiveDir pkgs;
          env.SKIA_BINARIES_URL = skiaBinariesUrl pkgs;
          env.ORT_LIB_LOCATION = "${pkgs.onnxruntime}/lib";
          env.ORT_PREFER_DYNAMIC_LINK = "1";
          # The release profile compiles each crate as one codegen unit, and
          # the window crate - mostly the Slint compiler's output, one
          # enormous module - then wants more memory than a sandboxed
          # builder has, and rustc is killed part way through ("rustc was
          # terminated by a deadly signal"). Sixteen units kept every piece
          # a size a builder could hold while the window crate was the size
          # it was; the director's power pass grew it again (Forge's own
          # page, the adapters' door, the stock and music panes rebuilt),
          # and at sixteen the single rustc was killed on its own - no
          # neighbour to blame, one crate, one process, dead at the same
          # line three times on this pull request. Thirty-two: every piece
          # half the size, the same cure this knob gave when it was a
          # builder's whole problem (jub0t/Concat#225); the thin LTO still
          # joins them, and the joiner is a linker, not a rustc.
          # https://github.com/jub0t/Concat/issues/225
          # The export queue and the Motion page grew the window crate
          # again (export.rs by five hundred lines, export.slint by nearly
          # two hundred), and on the merge that carried them (4a65a7b0)
          # the lone rustc was killed at thirty-two units and opt-level 1 -
          # the same signature, no neighbour, one process, dead fifty
          # minutes in. Sixty-four: every piece half the size again, the
          # cure this knob has given every time the module outgrew it.
          #
          # A correction this row now carries: an env var here has never
          # reached the window crate. Cargo lets a manifest's
          # [profile.release.package.concat] override beat
          # CARGO_PROFILE_RELEASE_CODEGEN_UNITS, so the units the window
          # crate actually compiled at were always the manifest's, and
          # this env var governed only the crates beside it. The ladder's
          # sixteen-thirty-two-sixty-four rungs on the window crate live
          # in src/Cargo.toml, which is where the next rung - one
          # hundred and twenty-eight, after the data-graphics layer and
          # the Geneva phases grew the module past sixty-four on the Nix
          # builder (04d16d9, swap and all) - was added. This env var
          # holds the rest of the workspace at sixty-four so no other
          # crate's rustc is the one that outgrows the builder either.
          env.CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "64";
          # And the crate beside its neighbours: at cargo's default width
          # the window crate's rustc ran beside slint's, motion's and the
          # mcp's at once, and after the interface grew three and a half
          # thousand lines the four together were more than the builder
          # held - the same kill the codegen units above once were, only
          # across processes, and the same cure ci.yml gives its own lanes:
          # one heavy thing at a time. The hook that runs the build passes
          # its own `-j $NIX_BUILD_CORES` on the command line - an env var
          # would be written over by it - but it runs this hook first, so
          # the cores themselves are what is narrowed. Two held while the
          # window crate was the size it was; the director's power pass
          # grew it again (Forge's own page, the adapters' door, the stock
          # and music panes rebuilt), and a pair of heavy rustcs was more
          # than the builder held a second time - the concat crate's was
          # killed beside a neighbour's ("rustc was terminated by a deadly
          # signal", twice on the same pull request). One: the window
          # crate's rustc now runs alone, and the wall time is the sum of
          # the lanes, which the job's two hours still holds with room.
          preBuild = ''
            export NIX_BUILD_CORES=1
          '';

          nativeBuildInputs = (nativeInputs pkgs) ++ [
            pkgs.wrapGAppsHook3
            pkgs.copyDesktopItems
          ];
          buildInputs = buildInputs pkgs;

          preFixup = ''
            gappsWrapperArgs+=(
              --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}
            )
          '';

          desktopItems = [
            (pkgs.makeDesktopItem {
              name = "concat";
              exec = "concat";
              icon = "concat";
              desktopName = "Concat";
              comment = "Free and open source video editor";
              startupWMClass = "concat";
              categories = [
                "AudioVideo"
                "AudioVideoEditing"
              ];
            })
          ];

          postInstall = ''
            install -Dm644 assets/concat_logo_512.png \
              $out/share/icons/hicolor/512x512/apps/concat.png
          '';

          meta = {
            description = "Free and open source video editor";
            homepage = "https://github.com/jub0t/Concat";
            license = pkgs.lib.licenses.agpl3Plus;
            platforms = systems;
            mainProgram = "concat";
          };
        };
      });

      devShells = eachSystem (pkgs: {
        default = pkgs.mkShell {
          packages =
            (nativeInputs pkgs)
            ++ (buildInputs pkgs)
            ++ (with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
            ]);

          env.SHERPA_ONNX_ARCHIVE_DIR = sherpaArchiveDir pkgs;
          env.SKIA_BINARIES_URL = skiaBinariesUrl pkgs;
          env.ORT_LIB_LOCATION = "${pkgs.onnxruntime}/lib";
          env.ORT_PREFER_DYNAMIC_LINK = "1";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibs pkgs);

          shellHook = ''
            echo "Concat: cd src && cargo run -p concat"
          '';
        };
      });

      formatter = eachSystem (pkgs: pkgs.nixfmt-rfc-style);
    };
}
