# Trunks — Guia operacional

> Para quem instala, configura e mantém um servidor Trunks. A referência completa de configuração é a secção 9 de `02-especificacao.md`; aqui está o que é preciso para pôr a correr e manter a correr.

## 1. Instalar

### Binário

Cada release publica arquivos para Linux, macOS e Windows (x86_64 e arm64), com checksums e instaladores:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/websublime/trunks/releases/latest/download/trunks-installer.sh | sh
trunks start            # development, SQLite em ./trunks.db, porta 8080
```

O mesmo binário é o servidor (`trunks start`) e o CLI de gestão (todos os outros comandos).

### Imagens

| Imagem | Conteúdo | Para |
|---|---|---|
| `ghcr.io/websublime/trunks:<versão>` | só o binário (distroless) | upstreams por HTTP |
| `ghcr.io/websublime/trunks:<versão>-stdio` | + Node.js 24 (`npx`) e uv com Python 3.13 (`uvx`) | upstreams stdio distribuídos como pacotes npm ou PyPI |

`latest` e `latest-stdio` acompanham a última release final. `linux/amd64` e `linux/arm64`.

Em ambas: o processo **não corre como root** (uid 65532 na distroless, 10001 na stdio), os dados ficam em `/data` (um volume) e `TRUNKS_DATABASE_URL` vem predefinido para `sqlite:///data/trunks.db`. A imagem distroless não tem shell nem `npx`: registar nela um upstream stdio falha com "failed to spawn".

### Compose

- `docker-compose.yaml` — uma pessoa, uma máquina: development, SQLite num volume, imagem `-stdio`, porta só em `127.0.0.1`.
- `docker-compose.prod.yaml` — enterprise sobre PostgreSQL. Copiar `.env.example` para `.env`, preencher e pôr um reverse proxy com TLS à frente (secção 4).

## 2. Modos

| | `development` | `enterprise` |
|---|---|---|
| Autenticação | nenhuma | tokens (ou SSO) em tudo |
| Quem vê o quê | toda a gente vê tudo | users e MCP clients só vêem o que lhes foi concedido |
| Inicialização | automática | `trunks init-server`, uma vez |
| `/metrics` | desligado por defeito | ligado por defeito |

**Development não tem controlo de acesso nenhum.** Só em `127.0.0.1` ou numa rede em que todos os que lá chegam sejam de confiança.

O modo fica gravado na base de dados na inicialização; arrancar depois com outro `TRUNKS_MODE` é um erro de arranque, não uma conversão.

Primeiro arranque em enterprise:

```bash
trunks --registry https://trunks.example.com init-server
```

Imprime **uma vez** o token do administrador e guarda-o na configuração do CLI (`~/.config/trunks/config.toml`, ou o caminho em `TRUNKS_CONFIG`). Perdê-lo sem haver outro administrador obriga a repor a base de dados.

## 3. Base de dados

| | SQLite | PostgreSQL |
|---|---|---|
| URL | `sqlite://caminho/trunks.db` (`sqlite:///abs/trunks.db` para caminho absoluto) | `postgres://user:pass@host:5432/db` |
| Para | uma pessoa, uma equipa pequena | enterprise |
| Cópia de segurança | `sqlite3 trunks.db ".backup copia.db"` com o servidor a correr; ou parar e copiar `trunks.db` **com** `-wal` e `-shm` | `pg_dump` |

- **Migrações** aplicam-se sozinhas no arranque e são aditivas. Fazer cópia de segurança antes de actualizar.
- **Uma instância por base de dados.** O catálogo (servidores, tools, grupos) vive em memória e só é recarregado pela instância onde a alteração é feita; outras instâncias sobre a mesma base de dados só a vêem depois de reiniciarem.
- **O URL do PostgreSQL leva a password.** Para não a pôr numa variável de ambiente, `TRUNKS_DATABASE_URL_FILE` aponta para um ficheiro com o URL (um secret de Docker ou de Kubernetes montado como ficheiro); quando está definido, prevalece sobre `TRUNKS_DATABASE_URL`.
- **A base de dados contém segredos em claro**: tokens de acesso de users e de MCP clients, credenciais dos upstreams (bearer, headers, `env`) e tokens OAuth. Proteger o ficheiro ou a instância e as cópias de segurança como se protege um cofre de passwords. Das sessões de browser só fica guardado um hash.

## 4. Rede

O Trunks fala HTTP simples. Em produção fica atrás de um reverse proxy que termina TLS.

| Definição | O que é | Em produção |
|---|---|---|
| `TRUNKS_BIND_HOST` / `TRUNKS_PORT` | onde escuta (vazio = todas as interfaces) | uma interface interna, ou a porta publicada só para o proxy |
| `TRUNKS_PUBLIC_URL` | o endereço visto de fora | `https://trunks.example.com` — decide o `Secure` do cookie, a origem aceite nas alterações feitas com sessão de browser e o redirect do SSO |
| `TRUNKS_ALLOWED_HOSTS` | cabeçalhos `Host` aceites nos endpoints MCP (protecção contra DNS rebinding); por defeito só loopback | o nome público; outro `Host` recebe 403 |

No proxy:

- **Não fazer buffering das respostas** de `/mcp` e `/v0/groups/*/mcp`: são streams (`text/event-stream`).
- Tempo de espera de leitura acima de `TRUNKS_REQUEST_TIMEOUT_SEC` (300 s por defeito), para ser o Trunks a responder 504 e não o proxy a cortar.
- Ligações keep-alive do proxy para o Trunks com tempo de inactividade **abaixo de 30 s**: o Trunks fecha ligações paradas há 30 s.
- Corpo dos pedidos: o Trunks recusa acima de 4 MiB (413).

Limites do próprio Trunks, para dimensionar à volta: 30 s para um cliente enviar o cabeçalho do pedido; 300 s por pedido REST e por pedido a um upstream (`TRUNKS_REQUEST_TIMEOUT_SEC`, `0` desliga); 30 s para o `initialize` de um upstream (`TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC`).

## 5. Upstreams

- **stdio**: o processo corre dentro da máquina ou do contentor do Trunks, com o utilizador do Trunks e com o ambiente do Trunks mais o `env` configurado. Só registar comandos em que se confia: quem pode registar servidores pode executar programas. Em enterprise, só administradores registam.
- `stateless` (por defeito) lança um processo por chamada; `stateful` mantém um aberto e reabre-o quando falha. Para servidores lançados por `npx`/`uvx`, `stateful` poupa o arranque em cada chamada (ver `07-desempenho.md`); `TRUNKS_SESSION_IDLE_TIMEOUT_SEC` fecha os que ficam parados.
- No fim da sessão o processo e tudo o que ele lançou são terminados.
- O stderr do processo aparece nos logs com `target = upstream.stderr`, com os valores de `env` configurados substituídos por `[redacted]`. Um segredo que o servidor obtenha por outra via e imprima fica no log tal como está.

## 6. Observar

- `GET /health` → 200 quando o processo está a servir. Não verifica upstreams.
- `GET /metrics` (Prometheus). Sem autenticação: não o publicar para fora.
  - `trunks_tool_calls_total{mcp_server_name,tool_name,outcome}` e `trunks_tool_call_latency_seconds` — chamadas e latência por tool;
  - `trunks_upstream_sessions_active{mcp_server_name,mode}` — sessões de upstream abertas neste momento;
  - `trunks_upstream_connect_errors_total{mcp_server_name}` — ligações a upstreams que falharam; a subir é o primeiro sinal de um upstream em baixo;
  - `trunks_build_info{version}` — a versão em execução.
- Logs em stdout/stderr. `TRUNKS_LOG` é um filtro `tracing` (`info` por defeito):
  - uma linha por resposta, com método, caminho (sem query string), estado, latência e tipo de quem pediu;
  - `TRUNKS_LOG=info,upstream.stderr=off` cala o stderr dos upstreams;
  - `debug` e `trace` registam argumentos e resultados das tools e o SQL — não usar em produção sem necessidade; o arranque avisa quando estão ligados.
- O estado de cada upstream (último erro de ligação, sessões abertas) está em `GET /api/dashboard/diagnostics`.

## 7. Parar, actualizar, repor

- `SIGTERM` (ou Ctrl-C): deixa de aceitar ligações, dá 10 s aos pedidos em curso, fecha as sessões dos upstreams e sai com 0. Streams MCP ainda abertos ao fim dos 10 s são cortados; os clientes voltam a ligar.
- **Actualizar**: cópia de segurança → trocar o binário ou a imagem → arrancar. As migrações correm no arranque.
- **Repor uma versão anterior**: repor também a cópia de segurança feita antes de actualizar. Uma versão antiga sobre um esquema mais novo não é suportada.
- Os tokens não mudam numa actualização: clientes MCP e CLI continuam configurados.
- `trunks version` mostra a versão do CLI e a do servidor. Uma versão com `+<commit>` não é um build de uma release.

## 8. Problemas comuns

| Sintoma | Causa provável |
|---|---|
| `403 server is not initialized` | enterprise sem `init-server` |
| arranque falha com o modo | `TRUNKS_MODE` diferente daquele com que a base de dados foi inicializada |
| cliente MCP recebe 403 logo na ligação | o `Host` que usa não está em `TRUNKS_ALLOWED_HOSTS` |
| `401 invalid MCP client token` | token de user usado num endpoint MCP (ou o contrário): são identidades diferentes |
| um user ou cliente "não vê nada" | falta um grant (`trunks grant …`) ou o servidor não está na allow-list do cliente |
| `tool '…' not found` numa tool que existe | está desactivada, fora do grupo, ou fora do alcance de quem chama — a resposta é a mesma de propósito |
| `failed to spawn 'npx'` | imagem distroless; usar a `-stdio` |
| `504` com `did not answer within …s` | o upstream não respondeu dentro de `TRUNKS_REQUEST_TIMEOUT_SEC` |
| respostas MCP cortadas ou só no fim | o proxy está a fazer buffering ou tem um tempo de espera curto |
| erros esporádicos "connection closed" num cliente com ligações persistentes | o cliente reutilizou uma ligação parada há mais de 30 s; baixar o tempo de inactividade do cliente ou do proxy |
