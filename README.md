# node-rust-windows
## Purpose
Find a way to make node based server class applications.

## Notes
I have fleshed out most of the support for using HTTP.sys APIs. Also, I have
gotten node ot response and serve content. Now, have to tinker with API signatures.

## Goals
- Use RUST to create .node plugins for windows system calls
  - Works very well using neon ✅
- Turn node.exe into a windows service without compiling or packaging.
  - POC ✅ | Works ✅ | On Hold ✅ | API Design ✅ | Implementation ✅
  - Use rust to build .node plugin to bridge windows service control API.
- Build tool to create .exe bindings for node entry points
  - POC ✅ | Works ✅ | On Hold ✅ | API Design | Implementation
  - Use rust to build .exe files that directly launch node.exe in a set pattern
  - Incorporate branding: Summmary Info, App ICON, Manifest, etc
- Expose system http api directly to node app (plug it into existing patterns seamlessly)
  - POC ✅ | Works ✅ | In Progress ✅ | API Design ✅ | Implementation (experimental)
