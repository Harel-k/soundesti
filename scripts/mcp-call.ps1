# Calls a tool on Soundesti's built-in MCP server (start the app with --mcp or enable
# OPTIONS > AI control). Prints the tool's text result; saves images to -ImageOut.
# Usage: scripts\mcp-call.ps1 get_state
#        scripts\mcp-call.ps1 set_tempo '{"bpm":140}'
#        scripts\mcp-call.ps1 screenshot '{}' -ImageOut shot.png
param([string]$Tool = "get_state", [string]$Arguments = "{}", [string]$ImageOut = "", [int]$Port = 47821)
$uri = "http://127.0.0.1:$Port/mcp"
$body = '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"' + $Tool + '","arguments":' + $Arguments + '}}'
$r = Invoke-RestMethod -Uri $uri -Method Post -ContentType "application/json" -Body $body
if ($r.error) { Write-Host "RPC error: $($r.error.message)"; exit 1 }
foreach ($c in $r.result.content) {
    if ($c.type -eq "text") { $c.text }
    elseif ($c.type -eq "image" -and $ImageOut) { [IO.File]::WriteAllBytes($ImageOut, [Convert]::FromBase64String($c.data)); "image saved to $ImageOut" }
}
if ($r.result.isError) { exit 1 }
