# Videnoa Controller

Videnoa Controller is the standalone, GPU-free coordination service for one or
more Videnoa workers. The web application is embedded in the executable; the
archive does not need a separate frontend directory, model bundle, or GPU
runtime.

## First Run

Extract the archive and work inside its root directory. Create the configured
input, output, data, and temporary directories before startup. Copy
`controller.example.toml` to an operator-owned location and replace its Linux
paths with absolute paths appropriate for the host. Windows configurations use
ordinary quoted absolute paths such as `D:\\Media\\Incoming`.

Generate the administrator password hash without placing the password on the
command line:

```bash
./videnoa-controller hash-password > /var/lib/videnoa-controller/admin-password.phc
```

On Windows PowerShell:

```powershell
.\videnoa-controller.exe hash-password | Set-Content -NoNewline C:\VidenoaController\admin-password.phc
```

Protect the hash file with operating-system permissions. The configuration
references that file by path and must never contain a password or PHC hash.

Start the service with the explicit configuration:

```bash
./videnoa-controller --config /etc/videnoa-controller.toml
```

```powershell
.\videnoa-controller.exe --config C:\VidenoaController\controller.toml
```

The default listener is `127.0.0.1:3001`. Open that address in a browser after
startup. Use a trusted reverse proxy with HTTPS when the service is reachable
outside the host; keep `secure_cookie = true` for HTTPS deployments. For an
explicit trusted HTTP-only network, set it to `false` and heed the startup
warning.

Useful smoke checks:

```bash
./videnoa-controller --version
curl --fail http://127.0.0.1:3001/api/health
```

The Controller stores its SQLite database under `data_root` and transient
downloads under `temp_root`. Back up the data directory and keep every worker's
Videnoa data directory persistent so task reconciliation remains safe.
