# Privacy Guarantee & Security Policy

## Offline Privacy Architecture

1. **Zero Network Calls**: The library performs no network I/O, analytics telemetry, or remote logging.
2. **Ephemeral Memory Processing**: Typed characters are evaluated in memory for suggestion lookup and immediately released.
3. **App Extension Isolation**: `RequestsOpenAccess` is set to `false` in iOS Custom Keyboard targets.

```xml
<key>NSExtension</key>
<dict>
    <key>NSExtensionAttributes</key>
    <dict>
        <key>RequestsOpenAccess</key>
        <false/>
    </dict>
</dict>
```
