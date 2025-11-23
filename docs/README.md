# ragrep Documentation

This directory contains comprehensive implementation guides and architecture documentation for ragrep.

## 📁 Files

### Architecture & Design
- **`../ARCHITECTURE.md`** - Complete architecture overview
  - Design decisions
  - File structure
  - Communication protocols
  - 4-phase implementation roadmap
  - Performance expectations

### Implementation Guides (Tutorial Style)

#### Phase 1: Server/Client Architecture
**`PHASE1_IMPLEMENTATION_GUIDE.md`** (35KB, 1335 lines)

Step-by-step guide to building the client/server architecture.

**What You'll Build**:
- Unix socket server keeping models loaded in memory
- Client that connects for fast queries
- Graceful fallback to standalone mode
- Process management (PID files, clean shutdown)

**7 Milestones**:
1. Define Protocol Types (tests included)
2. Build Server Skeleton (verifiable with `nc`)
3. Add Query Handling (real search results)
4. Build Client (10x faster queries!)
5. Add Fallback Logic (graceful degradation)
6. Process Management (PID files, Ctrl+C handling)
7. Integration Testing (end-to-end verification)

**Time**: ~1 week  
**Difficulty**: Intermediate  
**Lines of Code**: ~400

#### Phase 2: MCP Integration  
**`PHASE2_IMPLEMENTATION_GUIDE.md`** (35KB, 1353 lines)

Step-by-step guide to adding Model Context Protocol support.

**What You'll Build**:
- MCP server using rust-mcp-sdk
- `search_code` tool for AI assistants
- stdio transport for Claude Desktop
- Production error handling
- Real Claude integration

**7 Milestones**:
1. Add MCP Dependencies (rust-mcp-sdk)
2. Define Search Tool (`#[mcp_tool]` macro)
3. Implement MCP Handler (ServerHandler trait)
4. Add MCP Server Mode (`ragrep --mcp`)
5. Test with MCP Inspector (official testing tool)
6. Configure Claude Desktop (real AI usage!)
7. Production Polish (error handling, logging)

**Time**: ~1 week  
**Difficulty**: Intermediate-Advanced  
**Lines of Code**: ~300

---

## 🎓 For Junior Developers

These guides are written specifically for junior developers to follow step-by-step.

### Guide Features
- ✅ **Verifiable milestones** - Every step has a test
- ✅ **Copy-paste code** - Complete implementations provided
- ✅ **Expected outputs** - Know what success looks like
- ✅ **Troubleshooting** - Common issues and fixes
- ✅ **Learning notes** - Explanations of why, not just how

### How to Use These Guides

1. **Read the Background** - Understand the problem
2. **Review the Architecture Diagram** - Visualize what you're building
3. **Complete Each Milestone** - Step by step
4. **Verify at Each Step** - Run the tests/checks
5. **Troubleshoot if Needed** - Use the debugging sections
6. **Celebrate Completion!** - Each milestone is progress

### Recommended Order

```
Week 1: Phase 1
  ├─ Day 1-2: Milestones 1-3 (Protocol, Server, Queries)
  ├─ Day 3-4: Milestones 4-5 (Client, Fallback)
  └─ Day 5: Milestones 6-7 (Process Mgmt, Testing)

Week 2: Phase 2
  ├─ Day 1: Milestones 1-2 (Dependencies, Tool Definition)
  ├─ Day 2-3: Milestones 3-4 (Handler, Server Mode)
  ├─ Day 4: Milestone 5 (MCP Inspector Testing)
  └─ Day 5: Milestones 6-7 (Claude Desktop, Polish)
```

---

## 🧪 Testing Your Implementation

### After Phase 1

```bash
# Build
cargo build --release

# Test standalone mode
time ./target/release/rag "error handling"
# Should take ~7s

# Start server
./target/release/rag serve &

# Test client mode
time ./target/release/rag "error handling"
# Should take ~2s (63% faster!)

# Test fallback
pkill -f "rag serve"
time ./target/release/rag "error handling"  
# Should work (slowly) in standalone mode
```

### After Phase 2

```bash
# Start MCP server
./target/release/rag --mcp

# Test with MCP Inspector
# Visit: https://modelcontextprotocol.io/docs/tools/inspector
# Connect to your server
# Call search_code tool

# Test with Claude Desktop
# 1. Configure ~/.config/Claude/claude_desktop_config.json
# 2. Restart Claude Desktop
# 3. Ask: "Use ragrep to find error handling code"
# 4. Verify Claude uses the tool
```

---

## 📊 What You'll Learn

### Technical Skills

**Phase 1**:
- Async networking with Tokio
- Unix domain sockets
- Client/server architecture
- Process management
- Error handling patterns
- Performance optimization

**Phase 2**:
- Protocol integration (MCP)
- Procedural macros usage (`#[mcp_tool]`)
- AI tool development
- JSON Schema generation
- Result formatting for AI consumption

### Soft Skills

- Reading technical documentation
- Debugging async code
- Testing strategies
- Production thinking
- User experience design

---

## 🎯 Success Criteria

### Phase 1 Complete When:
- [ ] Server starts and handles connections
- [ ] Client queries are 10x faster with server
- [ ] Graceful fallback works without server
- [ ] PID file management works
- [ ] Ctrl+C stops server cleanly
- [ ] Integration tests pass

### Phase 2 Complete When:
- [ ] MCP server starts (`ragrep --mcp`)
- [ ] MCP Inspector can call search_code
- [ ] Claude Desktop shows ragrep connection
- [ ] Claude can search your code
- [ ] Error handling works gracefully
- [ ] Results formatted well for AI

---

## 🐛 Common Issues

### "Can't compile"
**Fix**: 
```bash
cargo clean
cargo build
```

### "Server won't start"
**Causes**:
- Models not found
- Database not indexed
- Port/socket already in use

**Fix**:
```bash
ragrep index  # Make sure indexed first
rm .ragrep/ragrep.sock  # Remove stale socket
```

### "Claude can't connect"
**Causes**:
- Wrong path in config
- Relative path instead of absolute
- Server not starting

**Fix**:
```bash
# Get absolute path
pwd
# Use full path like: /Users/you/project/target/debug/rag

# Test manually
/full/path/to/rag --mcp
```

---

## 📚 Additional Resources

### Official Documentation
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [rust-mcp-sdk](https://github.com/rust-mcp-stack/rust-mcp-sdk)
- [MCP Specification](https://modelcontextprotocol.io/specification/)

### Reference Implementations
- [rust-mcp-filesystem](https://github.com/rust-mcp-stack/rust-mcp-filesystem) - Production MCP server
- [mistral.rs](https://github.com/EricLBuehler/mistral.rs) - LLM with MCP

### Tools
- [MCP Inspector](https://modelcontextprotocol.io/docs/tools/inspector) - Test MCP servers
- [cargo-watch](https://crates.io/crates/cargo-watch) - Auto-rebuild on changes

---

## 🚀 Future Phases

### Phase 3: Git Integration (Planned)
- Auto-detect file changes using git
- Incremental reindexing
- Only reindex changed files
- ~250 lines of code

### Phase 4: Production Polish (Planned)  
- Comprehensive error handling
- Performance profiling
- Metrics and logging
- User documentation
- CI/CD setup
- Release v1.0

---

## 💡 Tips for Success

1. **Don't skip milestones** - Each builds on the previous
2. **Test at each step** - Catch issues early
3. **Read error messages carefully** - They're usually helpful
4. **Use `cargo check` often** - Faster than full build
5. **Keep notes** - Document what you learn
6. **Ask for help** - If stuck >30min, seek guidance
7. **Celebrate wins** - Each milestone is progress!

---

## 🎉 After Completion

Once you've finished both guides, you'll have:

- A production-ready code search tool
- Real-world async Rust experience
- Understanding of client/server architecture
- MCP protocol integration skills
- AI-accessible tool development experience
- Code you can show in interviews!

**Next Steps**:
1. Use ragrep in your daily development
2. Gather feedback from users
3. Iterate and improve
4. Consider contributing to the project
5. Share what you learned!

---

**Questions?** Check the troubleshooting sections in each guide, or review the architecture document for design context.

**Happy coding!** 🚀
