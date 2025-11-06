// Security Headers Script (Response)
// Adds comprehensive security headers to all responses

// Frame protection
headers["X-Frame-Options"] = "DENY";
headers["Content-Security-Policy"] = "frame-ancestors 'none'";

// Content type sniffing protection
headers["X-Content-Type-Options"] = "nosniff";

// XSS protection
headers["X-XSS-Protection"] = "1; mode=block";

// HTTPS enforcement (if using HTTPS)
if headers.contains("x-forwarded-proto") {
    if headers["x-forwarded-proto"] == "https" {
        headers["Strict-Transport-Security"] = "max-age=31536000; includeSubDomains; preload";
    }
}

// Referrer policy
headers["Referrer-Policy"] = "strict-origin-when-cross-origin";

// Permissions policy
headers["Permissions-Policy"] = "geolocation=(), microphone=(), camera=()";

// Remove server identification
headers.remove("server");
headers.remove("x-powered-by");

log_debug("Security headers added");

true



