use axum::{
    body::Body,
    http::{header, HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use crate::models::ApiResponse;

/// Check if the request accepts HTML based on the Accept header
pub fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|accept| accept.contains("text/html"))
        .unwrap_or(false)
}

/// Generate an HTML error page
fn generate_html_error_page(status_code: StatusCode, _message: &str, frontend_url: &str) -> String {
    let status_num = status_code.as_u16();
    let heading = match status_code {
        StatusCode::UNAUTHORIZED => "Unauthorized Access",
        StatusCode::FORBIDDEN => "Access Denied",
        StatusCode::NOT_FOUND => "Image Not Found",
        _ => "Error"
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} {} - GitmeRiz</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;600&display=swap" rel="stylesheet">
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: #19161D;
            color: #FAFAFA;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
            padding: 20px;
        }}
        .error-container {{
            text-align: center;
            max-width: 500px;
            width: 100%;
        }}
        .error-title {{
            font-size: 1.5rem;
            font-weight: 600;
            color: #FAFAFA;
            margin-bottom: 2rem;
        }}
        .btn-home {{
            display: inline-block;
            padding: 10px 24px;
            background: #211D27;
            color: #FAFAFA;
            text-decoration: none;
            border-radius: 6px;
            font-weight: 600;
            font-size: 0.95rem;
            transition: background 0.2s ease;
        }}
        .btn-home:hover {{
            background: #2a2630;
        }}
        @media (max-width: 640px) {{
            .error-title {{
                font-size: 1.25rem;
            }}
        }}
        @media (prefers-color-scheme: light) {{
            body {{
                background: #F4F3F6;
                color: #0F0F0F;
            }}
            .error-title {{
                color: #0F0F0F;
            }}
            .btn-home {{
                background: #dfdee6;
                color: #0F0F0F;
            }}
            .btn-home:hover {{
                background: #d0cfd6;
            }}
        }}
    </style>
</head>
<body>
    <div class="error-container">
        <h1 class="error-title">{} | {}</h1>
        <a href="{}" class="btn-home">← Back to Home</a>
    </div>
</body>
</html>"#,
        status_num,
        heading,
        status_num,
        heading,
        frontend_url
    )
}

/// Build an error response with content negotiation
/// Returns HTML for browsers (Accept: text/html) or JSON for API clients
pub fn build_error_response(
    status_code: StatusCode,
    message: &str,
    headers: &HeaderMap,
    frontend_url: &str,
) -> Response<Body> {
    if accepts_html(headers) {
        // Return HTML error page for browsers
        let html = generate_html_error_page(status_code, message, frontend_url);
        Response::builder()
            .status(status_code)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(html))
            .unwrap_or_else(|_| {
                // Fallback if HTML response building fails
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Internal server error"))
                    .unwrap()
            })
    } else {
        // Return JSON error for API clients
        (status_code, Json(ApiResponse::<()>::error(message))).into_response()
    }
}
