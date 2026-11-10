# Trunks — Desempenho

> Fase 7c do plano (`03-plano.md`). Números medidos, não objectivos: servem para saber a ordem de grandeza do que o gateway acrescenta e para detectar regressões grosseiras.

## 1. O que se mede

`cargo bench -p trunks --bench gateway` (`crates/trunks/benches/gateway.rs`) arranca o **binário real** (`trunks start`, build optimizado, SQLite) e mede-o de fora com um cliente MCP sobre Streamable HTTP, em loopback:

- **`tools/list`** com um catálogo de 500 tools — 10 upstreams stdio com 50 tools cada, registados pela API —, em modo development e em modo enterprise (cliente com acesso a tudo e cliente com acesso a 1 dos 10 servidores);
- **`tools/call`** de uma tool `echo`, através de sessões de upstream stateless e stateful, por stdio e por HTTP, ao lado da **mesma chamada feita directamente ao upstream**, sem gateway.

Os pedidos são sequenciais, numa única sessão MCP, com um cliente que reutiliza a ligação HTTP (como a generalidade dos clientes). Cada linha tem 20 pedidos de aquecimento não medidos. O upstream é o `fake-upstream` dos testes, também em build optimizado.

## 2. Resultados

Máquina: Intel Xeon @ 2.10 GHz, **2 núcleos lógicos** (VM partilhada), Linux x86_64. Rust 1.97.0. Commit da fase 7c. Latências em milissegundos, vistas pelo cliente.

### `tools/list`

| | n | média | p50 | p95 | p99 |
|---|---:|---:|---:|---:|---:|
| development — 512 tools, 197 KiB | 300 | 7,38 | 7,25 | 9,01 | 10,57 |
| enterprise, cliente com acesso a tudo — 500 tools, 195 KiB | 300 | 6,53 | 6,36 | 8,54 | 9,57 |
| enterprise, cliente com acesso a 1 de 10 servidores — 50 tools, 19 KiB | 300 | 1,28 | 1,27 | 1,48 | 1,57 |

### `tools/call` (`echo`)

| | n | média | p50 | p95 | p99 |
|---|---:|---:|---:|---:|---:|
| stdio, stateless (um processo por chamada) | 200 | 3,62 | 3,49 | 4,71 | 5,13 |
| stdio, stateful (sessão reutilizada) | 1000 | 0,49 | 0,51 | 0,65 | 0,72 |
| HTTP, stateless (uma sessão por chamada) | 500 | 4,85 | 4,47 | 6,53 | 8,03 |
| HTTP, stateful (sessão reutilizada) | 1000 | 0,79 | 0,79 | 1,29 | 1,59 |
| stdio, directo ao upstream | 1000 | 0,12 | 0,11 | 0,15 | 0,18 |
| HTTP, directo ao upstream | 1000 | 0,21 | 0,20 | 0,30 | 0,35 |

Em três execuções seguidas os p50 variaram entre si até cerca de 45 % (por exemplo, `tools/list` em development: 5,9 / 6,5 / 7,3 ms; HTTP stateful: 0,61 / 0,55 / 0,79 ms). A tabela é a terceira execução, não a melhor.

## 3. Leitura

- **O que o gateway acrescenta a uma chamada**, com a sessão de upstream já aberta: cerca de **0,4 ms** (0,5 ms contra 0,1 ms por stdio; 0,6–0,8 ms contra 0,2 ms por HTTP). Inclui o salto HTTP cliente → gateway, autenticação, resolução do nome e o reenvio.
- **Stateless custa o arranque da sessão em cada chamada**: ~3,5 ms por stdio (lançar o processo, `initialize`, chamar, encerrar) e ~4,5 ms por HTTP (nova sessão MCP: `initialize` + chamada). Com um servidor real lançado por `npx` ou `uvx`, o arranque do processo passa a dominar por ordens de grandeza — é para isso que existe `session_mode: stateful`.
- **`tools/list` é proporcional ao que se devolve**: ~6–7 ms para 500 tools (195 KiB de JSON a serializar, transportar e — do lado do cliente — interpretar), ~1,3 ms para as 50 tools que um cliente com allow-list vê. O filtro por permissões não tem custo visível: a listagem filtrada de enterprise não é mais lenta do que a de development.

## 4. O que o benchmark encontrou

Na primeira execução, **todas** as respostas MCP pequenas demoravam ~44 ms, com uma regularidade que nenhum trabalho real explica:

| | p50 antes | p50 depois |
|---|---:|---:|
| `tools/call`, stdio stateful | 44,0 | 0,5 |
| `tools/call`, stdio stateless | 44,0 | 3,5 |
| `tools/list`, 50 tools (19 KiB) | 44,0 | 1,3 |
| `tools/list`, 500 tools (195 KiB) | 5,2 | 6–7 |

Causa: as ligações aceites pelo servidor não tinham `TCP_NODELAY`. Uma resposta MCP é um cabeçalho HTTP, enviado de imediato, seguido de um evento pequeno quando o upstream responde; com o algoritmo de Nagle, o evento só saía depois de o cliente confirmar o cabeçalho, e o cliente atrasa essa confirmação ~40 ms (*delayed ACK*). As respostas grandes escapavam porque enchem segmentos. Acontece a partir do segundo pedido numa ligação reutilizada — que é como a maioria dos clientes HTTP funciona —, e já era assim com o `axum::serve`, antes do accept loop próprio.

Corrigido em `trunks_api::serve` (`set_nodelay(true)` em cada ligação aceite). O teste `small_mcp_answers_are_not_held_back_by_the_tcp_stack` (`crates/trunks/tests/server_process.rs`) corre no CI contra o binário real e falha se voltar a acontecer: exige que a mais rápida de 20 chamadas pequenas, numa ligação reutilizada, fique abaixo de 30 ms. Os testes anteriores não o apanhavam porque o cliente por omissão do `rmcp` abre uma ligação nova por pedido, e ligações novas são confirmadas de imediato.

## 5. Limites destes números

- Uma VM partilhada de 2 núcleos, onde correm ao mesmo tempo o cliente, o gateway e os upstreams. Noutra máquina os valores absolutos serão outros; as proporções devem manter-se.
- Um cliente, pedidos em série: mede **latência**, não débito nem comportamento sob concorrência.
- Loopback: não há rede. O efeito do `TCP_NODELAY` foi medido em Linux; não foi medido em macOS nem em Windows.
- O upstream é sintético e responde em microssegundos. Com upstreams reais, o tempo do upstream domina e a parcela do gateway é a da secção 3.
- SQLite. O backend não entra nestes caminhos: listagens e chamadas são servidas do catálogo em memória.

## 6. Reproduzir

```bash
cargo bench -p trunks --bench gateway
```

Compila o binário e o `fake-upstream` em modo optimizado e imprime as duas tabelas em Markdown. Não corre no CI (os runners partilhados dão números sem significado); `cargo clippy --all-targets` garante que continua a compilar.
