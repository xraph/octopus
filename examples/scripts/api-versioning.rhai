// API Versioning Script
// Automatically adds version headers and handles deprecation

// Detect API version from URI
if uri.starts_with("/api/v1") {
    headers["X-API-Version"] = "1.0";
    headers["X-API-Deprecated"] = "true";
    headers["X-Deprecation-Date"] = "2025-12-31";
    headers["X-Upgrade-Info"] = "Please upgrade to /api/v2";
    
    log_warn(`v1 API access: ${method} ${uri}`);
    
} else if uri.starts_with("/api/v2") {
    headers["X-API-Version"] = "2.0";
    headers["X-API-Status"] = "stable";
    
} else if uri.starts_with("/api/v3") {
    headers["X-API-Version"] = "3.0";
    headers["X-API-Status"] = "beta";
    headers["X-API-Warning"] = "Beta API - subject to change";
    
    log_info(`v3 beta API access: ${method} ${uri}`);
}

// Add standard API headers
headers["X-API-Gateway"] = "Octopus";
headers["X-API-Docs"] = "https://docs.example.com/api";

true



