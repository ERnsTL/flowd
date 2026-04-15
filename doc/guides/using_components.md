## Using Components

1. Find the component on a Git service like Github, GitLab, CodeBerg etc. Repositories usually have "flowd-" in their name. Each repository constitutes a Cargo crate; it can contain one or more components.
2. Add the crate repository to your Cargo.toml like so, to make it known to Cargo:
  > flowd-bla = {
  >   git = "https://github.com/org/flowd-bla.git",
  >   rev = "a1b2c3d4e5f6",
  >   branch = "0.4"
  > }
  Alternatively, via submodules which also references a specific commit:
  > git submodule add https://github.com/org/flowd-bla components/bla
  and add into Cargo.toml like so:
  > flowd_bla = { path = "components/bla" }
3. Add all components contained in the crate repository to your flowd.build.toml, to have it built into flowd. Explanation in the included flowd.build.toml file. The component repository probably has a ready block for copy-paste in its README.
4. Build flowd as usual using ```cargo build --release```.
5. For your project, you can also commit Cargo.lock to have it build reproducibly.
