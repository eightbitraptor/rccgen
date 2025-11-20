# rccgen

A small command line utility that generates `compile_commands.json` files for Make/autotools based C projects. Designed to replace a `bear && compdb` based workflow.

when run from the root directory of your C project it will run `make -n -B` to discover compilation commands, and parse the output to extract all compilation flags, discover and include header files, and generate `compile_commands.json` in the current directory.

This has been tested on macOS and Linux using the `ruby/ruby` source tree as a test case.

## Features

- parses the output of `make -n -B`
- Includes header files
- Full support for gcc and clang compilers

## Anti-features

- Does not support compiler wrappers like `distcc` or `ccache`
- Only supports Make/autotools
- No thought given to cross-compilation
- Or multiarch
- Or Windows
- Or any other compilers other than gcc and clang
