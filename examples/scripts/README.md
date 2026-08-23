# Octopus Script Examples

Collection of ready-to-use Rhai scripts for common API gateway tasks.

## Scripts

### Request Processing

| Script | Purpose | Execution Time | Use When |
|--------|---------|----------------|----------|
| `request-logger.rhai` | Log all requests with IDs | ~15μs | Debugging, auditing |
| `api-versioning.rhai` | Add version headers, handle deprecation | ~10μs | API lifecycle management |
| `request-validation.rhai` | Validate headers and auth | ~20μs | Input validation |

### Response Processing

| Script | Purpose | Execution Time | Use When |
|--------|---------|----------------|----------|
| `security-headers.rhai` | Add security headers | ~10μs | All production APIs |
| `response-transform.rhai` | Wrap responses in envelope | ~30μs | API standardization |

## Usage

### Option 1: Reference in Config

```yaml
middleware:
  - type: script
    enabled: true
    config:
      language: rhai
      file: examples/scripts/request-logger.rhai
      on_request: true
```

### Option 2: Copy to System Path

```bash
# Copy scripts to system location
sudo mkdir -p /etc/octopus/scripts
sudo cp examples/scripts/*.rhai /etc/octopus/scripts/

# Reference in config
middleware:
  - type: script
    config:
      file: /etc/octopus/scripts/request-logger.rhai
```

### Option 3: Multiple Scripts

```yaml
middleware:
  # 1. Log all requests
  - type: script
    config:
      file: examples/scripts/request-logger.rhai
      on_request: true
  
  # 2. Validate requests
  - type: script
    config:
      file: examples/scripts/request-validation.rhai
      on_request: true
  
  # 3. Add version headers
  - type: script
    config:
      file: examples/scripts/api-versioning.rhai
      on_request: true
  
  # 4. Add security headers to responses
  - type: script
    config:
      file: examples/scripts/security-headers.rhai
      on_response: true
  
  # 5. Transform responses
  - type: script
    config:
      file: examples/scripts/response-transform.rhai
      on_response: true
```

## Performance

All scripts execute in **<100μs**:

```
request-logger:        ~15μs
api-versioning:        ~10μs
request-validation:    ~20μs
security-headers:      ~10μs
response-transform:    ~30μs
────────────────────────────
Total overhead:        ~85μs
```

## Customization

### Modify Existing Scripts

```bash
# Edit script
vi examples/scripts/request-logger.rhai

# Reload gateway (no restart needed with hot reload)
curl -X POST http://localhost:9090/admin/reload
```

### Create New Scripts

```bash
# Copy template
cp examples/scripts/request-logger.rhai my-custom-script.rhai

# Edit
vi my-custom-script.rhai

# Test
octopus-cli test-script my-custom-script.rhai
```

## Testing

### Manual Test

```bash
# Start gateway with script
cargo run --example quickstart

# Send test request
curl -v http://localhost:8080/api/v1/test
```

### Check Logs

```bash
# View script logs
tail -f /var/log/octopus/gateway.log | grep script_log
```

## Troubleshooting

### Script Not Running

1. Check syntax: `rhai my-script.rhai`
2. Verify path in config
3. Check `enabled: true`
4. Review error logs

### Performance Issues

1. Reduce script complexity
2. Cache expensive operations
3. Use native middleware for critical path
4. Profile with metrics

## Best Practices

1. **Keep scripts simple** - Complex logic → native plugins
2. **Handle errors** - Always return boolean
3. **Log appropriately** - Info/warn only, not debug in prod
4. **Test thoroughly** - Validate all code paths
5. **Version control** - Track script changes in git

## More Examples

See the [Scripting Guide](../../docs/SCRIPTING_GUIDE.md) for:
- Inline scripts
- Advanced patterns
- API reference
- Troubleshooting

---

**Need help?** Check the [main README](../../README.md) or open an issue.




