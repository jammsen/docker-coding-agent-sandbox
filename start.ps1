<#
.SYNOPSIS
    Build and start the agentic harness sandbox (Docker Desktop / Windows).

.DESCRIPTION
    Equivalent of start.sh for Windows users running Docker Desktop.
    Builds the image and starts the sandbox container with service ports mapped.

.PARAMETER Tool
    Directly select which tool to launch (opencode, omp, ...).
    Skips the interactive tool-selection menu inside the container.

.PARAMETER BuildArgs
    Extra arguments forwarded to `docker compose build` (e.g. --no-cache).

.EXAMPLE
    .\start.ps1
    .\start.ps1 --Tool opencode
    .\start.ps1 --BuildArgs --no-cache
    .\start.ps1 --Tool omp --BuildArgs --no-cache
#>
[CmdletBinding()]
param(
    [string]$Tool = "",

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$BuildArgs = @()
)

$ErrorActionPreference = 'Stop'

# Verify Docker is available
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Error "docker not found. Please install Docker Desktop: https://www.docker.com/products/docker-desktop/"
    exit 1
}

# Build
if ($BuildArgs.Count -gt 0) {
    docker compose build @BuildArgs
} else {
    docker compose build
}

if ($LASTEXITCODE -ne 0) {
    Write-Error "docker compose build failed (exit $LASTEXITCODE)"
    exit $LASTEXITCODE
}

# Run
if ($Tool) {
    docker compose run --rm --service-ports -e "DEFAULT_TOOL=$Tool" sandbox
} else {
    docker compose run --rm --service-ports sandbox
}

exit $LASTEXITCODE
