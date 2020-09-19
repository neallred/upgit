#!/usr/bin/env bash
# Thanks https://wapl.es/rust/2019/02/17/rust-cross-compile-linux-to-macos.html
# Thanks http://alwayscoding.ca/momentos/2016/05/08/cross-compilation-to-osx-with-rust
# Thanks https://users.rust-lang.org/t/help-on-cross-compiling-openssl-to-armhf/40871
compile_path="$(pwd)/osxcross/target/bin:$HOME/.cargo/bin:$PATH"
target="x86_64-apple-darwin"
my_cc=o64-clang
my_cxx=o64-clang

PATH="$compile_path" CC=$my_cc CXX=$my_cxx LIBZ_SYS_STATIC=1 cargo build --target $target
PATH="$compile_path" CC=$my_cc CXX=$my_cxx LIBZ_SYS_STATIC=1 cargo build --release --target $target
