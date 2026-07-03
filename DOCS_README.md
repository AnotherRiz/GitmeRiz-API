# Documentation Overview

This document explains the purpose of each documentation file in this project.

## Core Documentation

### README.md 📖
**Purpose:** Project overview, setup instructions, and getting started guide  
**Audience:** Developers setting up the project for the first time  
**When to read:** Before starting development

### AGENTS.md 🤖
**Purpose:** Rules, conventions, and guidelines for AI agents and developers  
**Audience:** AI agents (like me!) and human developers  
**When to read:** Before making code changes  
**Key content:**
- Code style conventions
- Authentication & authorization rules
- API design principles
- Security best practices
- Database conventions
- Git workflow

### api-docs.md 📡
**Purpose:** Complete API endpoint documentation  
**Audience:** Frontend developers and API consumers  
**When to read:** When integrating with the API  
**Key content:**
- All endpoint specifications
- Request/response examples
- Authentication requirements
- Error codes and messages

## Optimization Documentation

### OPTIMIZATION_ROADMAP.md 🚀
**Purpose:** Historical record of all performance optimizations  
**Audience:** Developers wanting to understand optimization decisions  
**Status:** **KEEP** - Valuable historical reference  
**Key content:**
- 3 major optimizations implemented:
  1. Single Decode + Cascading Resize (2x faster CPU)
  2. Parallel Disk I/O (10-20x faster file writes)
  3. Background Processing (4.3x faster perceived speed)
- Performance metrics (before/after)
- Implementation notes and commits
- Recommended future enhancements

### FRONTEND_INTEGRATION_GUIDE.md 🎨
**Purpose:** Practical guide for integrating with background processing  
**Audience:** Frontend developers  
**Status:** **KEEP** - Essential for frontend integration  
**Key content:**
- Breaking changes (202 Accepted vs 201 Created)
- Step-by-step integration instructions
- Complete React and Vue examples
- Status polling implementation
- Error handling best practices
- Testing checklist

## Documentation Guidelines

### When to Update

**AGENTS.md:** Update when:
- Adding new conventions or rules
- Changing authentication/authorization logic
- Modifying API patterns
- Adding security requirements

**api-docs.md:** Update when:
- Adding new endpoints
- Changing request/response formats
- Modifying authentication
- Changing status codes

**OPTIMIZATION_ROADMAP.md:** Update when:
- Implementing major performance optimizations
- Adding performance metrics
- Documenting architectural decisions

**FRONTEND_INTEGRATION_GUIDE.md:** Update when:
- Changing upload API behavior
- Adding new frontend-facing endpoints
- Modifying status tracking logic

### Documentation Hierarchy

```
1. README.md (Start here)
   ↓
2. AGENTS.md (Learn conventions)
   ↓
3. api-docs.md (Understand APIs)
   ↓
4. OPTIMIZATION_ROADMAP.md (Learn optimizations)
   ↓
5. FRONTEND_INTEGRATION_GUIDE.md (Frontend integration)
```

## Quick Reference

**New to the project?**  
Start with: `README.md` → `AGENTS.md`

**Building frontend?**  
Read: `api-docs.md` → `FRONTEND_INTEGRATION_GUIDE.md`

**Understanding performance?**  
Read: `OPTIMIZATION_ROADMAP.md`

**Contributing code?**  
Follow: `AGENTS.md` conventions

**AI agent working on this project?**  
Follow: `AGENTS.md` strictly, reference other docs as needed

## Document Status

| Document | Status | Last Updated | Keep/Remove |
|----------|--------|--------------|-------------|
| README.md | Active | - | ✅ Keep |
| AGENTS.md | Active | - | ✅ Keep |
| api-docs.md | Active | 2026-07-03 | ✅ Keep |
| OPTIMIZATION_ROADMAP.md | Complete | 2026-07-03 | ✅ Keep (Historical) |
| FRONTEND_INTEGRATION_GUIDE.md | Active | 2026-07-03 | ✅ Keep |
| DOCS_README.md | Active | 2026-07-03 | ✅ Keep |

---

**Note:** All documentation is in English except code comments which may be in Indonesian based on context and audience.
