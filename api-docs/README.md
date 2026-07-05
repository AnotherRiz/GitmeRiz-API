# GitmeRiz API Documentation

REST API for the GitmeRiz platform, built with Rust (Axum + MySQL). It provides
JWT authentication and role-based access control over six resources: Gallery,
Video, Audio, Blog, Notes, and Clipboard.

- **Base URL**: `http://localhost:3000`
- **Content-Type**: `application/json` (media uploads use `multipart/form-data`)
- **Auth**: JWT delivered via an `auth_token` HttpOnly cookie (set on login) or an
  `Authorization: Bearer <token>` header. The cookie takes priority.

## Documentation Index

| Document | Description |
|----------|-------------|
| [Conventions](conventions.md) | Response envelope, status codes, authentication flow, roles & permissions, file storage |
| [Authentication](authentication.md) | Register, login, logout |
| [Users](users.md) | User management endpoints |
| [Gallery](gallery.md) | Image upload, background processing, thumbnails, previews, pinning |
| [Video](video.md) | Video upload and management |
| [Audio](audio.md) | Audio upload and management |
| [Blog](blog.md) | Blog post CRUD |
| [Notes](notes.md) | Personal notes CRUD |
| [Clipboard](clipboard.md) | Clipboard items CRUD |
| [Health](health.md) | Health check endpoint |
| [Endpoint Summary](endpoint-summary.md) | Complete table of all endpoints |

## Quick Start

1. Register an account: `POST /register`
2. Log in to get a token/cookie: `POST /login`
3. Use the token/cookie for protected endpoints.

See [Authentication](authentication.md) for details.

## Overview

The API is organized around six core resources plus authentication and user
management. All responses use a consistent JSON envelope (see
[Conventions](conventions.md)). Media uploads (Gallery, Video, Audio) are stored
on disk and tracked in the database.

Gallery uploads use **background processing**: the upload endpoint returns
`202 Accepted` immediately, then generates thumbnails and previews asynchronously.
See [Gallery](gallery.md) for the full workflow.
