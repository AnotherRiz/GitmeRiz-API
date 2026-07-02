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
fn generate_html_error_page(status_code: StatusCode, message: &str, frontend_url: &str) -> String {
    let status_num = status_code.as_u16();
    let (heading, description) = match status_code {
        StatusCode::UNAUTHORIZED => (
            "Unauthorized Access",
            "You need to be logged in to view this image."
        ),
        StatusCode::FORBIDDEN => (
            "Access Denied",
            "You don't have permission to access this image."
        ),
        StatusCode::NOT_FOUND => (
            "Image Not Found",
            "The image you're looking for doesn't exist or has been removed."
        ),
        _ => (
            "Error",
            "An error occurred while processing your request."
        )
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} - GitmeRiz</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;600;700;800&display=swap" rel="stylesheet">
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
            line-height: 1.6;
        }}
        .error-container {{
            text-align: center;
            max-width: 600px;
            width: 100%;
        }}
        .error-code {{
            font-size: 8rem;
            font-weight: 800;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
            background-clip: text;
            margin-bottom: 1rem;
            line-height: 1;
        }}
        .error-heading {{
            font-size: 2rem;
            font-weight: 700;
            color: #FAFAFA;
            margin-bottom: 1rem;
        }}
        .error-description {{
            font-size: 1.125rem;
            color: #a0a0a0;
            margin-bottom: 2rem;
            line-height: 1.7;
        }}
        .error-message {{
            background: #211D27;
            border: 1px solid #2a2630;
            border-radius: 8px;
            padding: 1rem 1.5rem;
            margin-bottom: 2rem;
            color: #b0b0b0;
            font-size: 0.9rem;
        }}
        .btn-home {{
            display: inline-block;
            padding: 12px 32px;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: #FAFAFA;
            text-decoration: none;
            border-radius: 8px;
            font-weight: 600;
            font-size: 1rem;
            transition: all 0.2s ease;
            box-shadow: 0 4px 14px 0 rgba(102, 126, 234, 0.4);
        }}
        .btn-home:hover {{
            transform: translateY(-2px);
            box-shadow: 0 6px 20px 0 rgba(102, 126, 234, 0.5);
        }}
        .btn-home:active {{
            transform: translateY(0);
        }}
        @media (max-width: 640px) {{
            .error-code {{
                font-size: 5rem;
            }}
            .error-heading {{
                font-size: 1.5rem;
            }}
            .error-description {{
                font-size: 1rem;
            }}
        }}
        @media (prefers-color-scheme: light) {{
            body {{
                background: #F4F3F6;
                color: #0F0F0F;
            }}
            .error-heading {{
                color: #0F0F0F;
            }}
            .error-message {{
                background: #dfdee6;
                border: 1px solid #d0cfd6;
                color: #4a4a4a;
            }}
            .btn-home {{
                color: #FAFAFA;
            }}
        }}
    </style>
</head>
<body>
    <div class="error-container">
        <div class="error-code">{}</div>
        <h1 class="error-heading">{}</h1>
        <p class="error-description">{}</p>
        <div class="error-message">{}</div>
        <a href="{}" class="btn-home">← Back to Home</a>
    </div>
</body>
</html>"#,
        heading,
        status_num,
        heading,
        description,
        message,
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
