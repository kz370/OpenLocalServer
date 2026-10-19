# Run OLS Update Test Server
$python = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $python) {
    $python = (Get-Command python3 -ErrorAction SilentlyContinue).Source
}
if (-not $python) {
    Write-Error "Python is not installed or not in PATH."
    exit 1
}

& $python "$PSScriptRoot\serve.py"
