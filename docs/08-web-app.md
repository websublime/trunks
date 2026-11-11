# Trunks — Web app

> Versão 1.0 (2026-11-11). Definição da web app (Fase 6c). Substitui a definição anterior
> ("SPA em Lit + Vite"): a app passa a ser um site **Astro** com componentes **`line://ui`**,
> sem framework de UI (sem React, Vue, Svelte ou equivalente). Decisão D18 em `01-produto.md`.

## 1. O que é e o que não é

A web app é a consola do Trunks: mostra o que o gateway tem e, a quem pode, deixa geri-lo. É
servida pelo próprio binário (`trunks-dashboard`, `rust-embed`) e fala só com a API HTTP que
já existe: lê de `/api/dashboard/*`, altera por `/api/v0/*`, autentica-se em `/auth/*`.

Não é um segundo backend. Não há servidor Node em produção, não há SSR, e a app não tem
nenhum endpoint seu: tudo o que mostra vem de contratos que o CLI também usa.

## 2. Restrições que decidem o resto

| Restrição | Consequência |
|---|---|
| Um binário, sem runtime Node | Astro com `output: 'static'`: o build produz ficheiros, o Rust serve-os |
| O conteúdo depende de quem pede (D14) | Nenhum dado no HTML: as páginas são cascas estáticas; os dados chegam no browser, pela API, com a sessão de quem está a ver |
| `line://ui` é o design system (D2) | Custom elements `line-*` usados directamente no markup; tokens `--line-*`; dark-first |
| Sem React nem outro framework de UI | Estrutura em componentes `.astro` (zero JS); interactividade em custom elements |
| Sem SSE (D4) | Vistas "ao vivo" por polling, nunca por stream |
| Contratos estáveis (`02-especificacao.md`) | A app não inventa shapes: os tipos TypeScript são gerados dos tipos Rust |

## 3. Stack

| Camada | Escolha | Notas |
|---|---|---|
| Site | **Astro 7** (`output: 'static'`, `build.format: 'directory'`) | Vite 8 por baixo; precisa de Node 22 LTS para construir (só para construir) |
| Componentes de UI | **`line://ui`** (`@websublime/line-*`, Lit + Zag.js) | registados no browser por um `<script>` que os importa; sem integração Astro para Lit (a `@astrojs/lit` está descontinuada desde o Astro 5 e não faz falta: não há SSR de componentes) |
| Componentes da app | Custom elements próprios, prefixo `trunks-*`, escritos em **Lit** | o Lit já vem com a `line://ui`; são eles que vão buscar dados e os mostram |
| Linguagem | TypeScript estrito; `astro check` no CI | |
| Estilo | tokens `--line-*` + CSS com âmbito do Astro | sem Tailwind, sem CSS-in-JS |
| Estado | nenhum gestor de estado | cada página é um documento; o estado vive no elemento que o usa e no URL |
| Dados | módulo `api` tipado sobre `fetch` | secção 6 |
| Testes | Playwright contra o binário real; `vitest` para o módulo `api` | secção 10 |

O que fica de fora de propósito: React/Preact/Vue/Svelte/Solid, router de cliente, gestor de
estado global, biblioteca de data fetching, framework de CSS.

## 4. Arquitectura

```text
web/app/
├── astro.config.mjs
├── src/
│   ├── pages/            uma página por ecrã (ficheiros .astro; routing por ficheiros)
│   ├── layouts/          casca comum: navegação, cabeçalho, área de conteúdo
│   ├── components/       componentes .astro sem JS (estrutura, tipografia, estados vazios)
│   ├── elements/         custom elements trunks-* (Lit): tabelas com dados, formulários, diálogos
│   ├── api/              cliente tipado: client.ts, types.gen.ts (gerado), errors.ts
│   └── styles/           tokens e base
└── tests/                Playwright
```

Divisão de responsabilidades:

- **Página `.astro`**: escolhe o layout, declara que elementos o ecrã tem. Não tem lógica.
- **Componente `.astro`**: markup e estilo reutilizáveis, sem JavaScript no browser.
- **Elemento `trunks-*`**: uma peça com dados ou interacção (por exemplo `trunks-server-list`).
  Usa os `line-*` por dentro, chama o módulo `api`, trata os seus estados (a carregar, vazio,
  erro, sem permissão).
- **`line-*`**: primitivas (botão, diálogo, campo). A app não as reimplementa nem as embrulha
  em wrappers genéricos.

Uma página é, portanto, HTML estático com dois ou três elementos que se preenchem sozinhos.
A navegação entre ecrãs é navegação normal de documentos, com `prefetch` do Astro. O
`<ClientRouter />` (view transitions) fica fora da primeira versão: obriga a reinicializar
elementos e scripts a cada troca e não resolve nenhum problema que tenhamos.

## 5. Rotas

A app vive na raiz. Estes prefixos são do servidor e nenhuma página os pode usar:
`/api`, `/auth`, `/mcp`, `/v0`, `/health`, `/metadata`, `/metrics`, `/init`. Os ficheiros
gerados pelo Astro ficam em `/_astro/`.

| Rota | Ecrã | Quem |
|---|---|---|
| `/` | overview | todos |
| `/login/` | entrada: token e, se configurado, "Sign in with SSO" | enterprise, sem sessão |
| `/servers/` | servidores | todos (o que podem ver) |
| `/servers/detail/?name=` | um servidor: tools, prompts, resources, estado | todos |
| `/servers/new/` | registar, incluindo OAuth upstream | admin |
| `/tools/`, `/prompts/`, `/resources/` | catálogo, com filtro por servidor | todos |
| `/groups/`, `/groups/detail/?name=`, `/groups/new/`, `/groups/edit/?name=` | tool groups | todos / admin para alterar |
| `/clients/` | MCP clients: os meus e os das minhas teams; todos para admin | enterprise |
| `/users/`, `/teams/`, `/teams/detail/?name=`, `/access/` | identidades e grants | enterprise, admin (teams: também maintainers) |
| `/diagnostics/` | diagnóstico da instalação | admin |
| `/account/` | quem sou, o que posso usar, sair | enterprise |

**Páginas de detalhe usam query string**, não `/servers/{name}`. Num build estático todas as
rotas têm de ser conhecidas no build, e os nomes dos servidores não são. A alternativa (URLs
limpos com uma tabela de reescrita no Rust) obrigava a manter essa tabela a par das páginas e
deixava de funcionar igual no `astro dev`.

Uma rota que o utilizador não pode usar não aparece na navegação; aberta directamente, mostra
"sem permissão" a partir do 403/404 da API. A app nunca decide acessos: só reflecte o que a
API responde.

## 6. Dados

- `api/client.ts`: um wrapper de `fetch` com `credentials: 'same-origin'`, que descodifica o
  envelope de erro (`{error, code?}`) para um tipo `ApiError` e trata dois casos em comum:
  401 → ir para `/login/?return_to=…`; 403 "server is not initialized" → ecrã próprio.
- `api/types.gen.ts`: **gerado a partir dos tipos Rust** de `trunks-types` e
  `trunks-core::dashboard` (derive `ts-rs` atrás de uma feature), com um teste que falha quando
  o ficheiro deixa de coincidir, como já acontece com a referência do CLI. A app não escreve
  shapes à mão.
- Leituras em `/api/dashboard/*`; mutações em `/api/v0/*`. Depois de uma mutação, o elemento
  volta a ler: não há cache partilhada para invalidar.
- Vistas que mudam sozinhas (overview, estado dos upstreams, autorização OAuth pendente):
  polling com intervalo fixo, parado quando o separador não está visível.

## 7. Autenticação

- **Development**: sem login. `GET /auth/config` diz `login_required: false` e a app entra.
- **Enterprise**: `GET /auth/config` decide o que a página de login mostra.
  - Token: `POST /auth/token` troca-o por um cookie de sessão `HttpOnly`. **O token nunca é
    guardado no browser** (nem `localStorage`, nem memória para lá do pedido).
  - SSO: ligação para `/auth/login?return_to=…`.
  - Sair: `POST /auth/logout`.
- As mutações com cookie exigem `Origin` igual ao URL público (já implementado no servidor);
  a app não precisa de token CSRF.
- Um token mostrado uma única vez (criar user, criar ou rodar MCP client) aparece num diálogo
  com botão de copiar e não sobrevive ao fecho do diálogo.

## 8. Servir (lado Rust)

`trunks-dashboard` embebe `dist/`. As regras de resolução, em `trunks-api`:

1. `GET` de um caminho que corresponde a um ficheiro → esse ficheiro.
2. Senão, `<caminho>/index.html` (formato `directory` do Astro; `/servers` e `/servers/`
   servem o mesmo).
3. Senão, `404.html` com estado 404.

- Cache: `/_astro/*` tem nomes com hash → `Cache-Control: public, max-age=31536000, immutable`;
  HTML → `no-cache`, com `ETag`.
- Cabeçalhos de segurança nas respostas estáticas: `X-Content-Type-Options: nosniff`,
  `Referrer-Policy: same-origin` e uma `Content-Security-Policy` estrita, enviada pelo
  servidor: `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:;
  font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self';
  frame-ancestors 'none'`. Nada inline: o build do Astro não embute scripts nem estilos nas
  páginas (`inlineStylesheets: 'never'`, `assetsInlineLimit: 0`) e
  `web/app/scripts/check-dist.mjs` falha o build se aparecer um `<script>` sem `src`, um
  `<style>`, um atributo `style` ou um handler inline. A política vai no cabeçalho, e não na
  `<meta>` do Astro, porque só assim cobre `frame-ancestors` e não depende de hashes.
- **Os estáticos são públicos, mesmo antes da inicialização**: são a casca da app, igual para
  toda a gente e sem dados; a página de login tem de abrir sem sessão e a página "server not
  initialized" tem de abrir antes de haver seja o que for. Tudo o que é dado continua atrás
  da API (que responde 403 enquanto o servidor não está inicializado).
- Os prefixos do servidor (secção 5) nunca recebem uma página: um caminho desconhecido lá
  responde 404 sem corpo. A app só se lê com `GET` e `HEAD`.
- Sem `dist/` (build do servidor sem Node), `GET /` responde 404, como hoje.
- Sem compressão por agora: o build inteiro tem cerca de 230 KB. Se vier a existir, aplica-se só aos estáticos, nunca aos endpoints MCP, que são streams.

## 9. Construir e desenvolver

- `cargo xtask build-dashboard`: `npm ci`, `astro check`, `vitest`, `astro build` e a
  verificação da CSP em `web/app`, com a saída copiada para `crates/trunks-dashboard/dist`.
  Precisa de Node 22 LTS.
- `cargo xtask gen-types` escreve `web/app/src/api/types.gen.ts` a partir dos tipos Rust
  (`ts-rs`, atrás da feature `ts` de `trunks-types` e `trunks-core`); um teste do `xtask`
  falha quando o ficheiro deixa de coincidir.
- O release (cargo-dist, `.github/build-setup.yml`) e as imagens de contentor
  (estágio `web` do `Dockerfile`) constroem a app antes do `cargo build`; um binário de
  release sem a app embebida é um erro do pipeline, não uma variante. Um `cargo build` local
  sem `build-dashboard` continua a compilar e serve 404 em `/`.
- Desenvolvimento: `astro dev` (porta 4321) com proxy do Vite para um `trunks start` local nos
  prefixos da secção 5. Mesma origem para o browser, logo o cookie e a verificação de
  `Origin` funcionam como em produção.
- Versões fixadas (`package-lock.json` no repositório); Astro actualiza-se num PR próprio.

## 10. Testes

| Nível | Ferramenta | O que garante |
|---|---|---|
| Tipos | `astro check`; teste de deriva de `types.gen.ts` | a app compila contra os contratos reais |
| Módulo `api` | `vitest` | envelope de erro, 401 → login, 403 por inicializar |
| Fumo, ponta a ponta | Playwright contra o **binário real** com a app embebida | development: registar um servidor, ver tools, desactivar uma, criar um grupo; enterprise: login por token, o que um user sem grants vê (nada), o que vê com um grant, criar um MCP client e ver o token uma vez |
| Acessibilidade | `axe` nas páginas do fumo | sem violações AA; depende também dos tokens de contraste da `line://ui` |
| Servir | testes de `trunks-api` | resolução de caminhos, 404, cabeçalhos de cache e de segurança |

O job `web` do CI corre os três primeiros e o fumo; a conformidade da Fase 7 ganha a célula
"web app".

## 11. O que a app precisa da `line://ui`

Estado da biblioteca visto no Storybook publicado em 2026-11-11. **Existem**: `alert`
(com regiões live e receita de dismiss), `badge`, `button` (com toggle e estado loading),
`button-group`, `chip` (com a receita de barra de filtros), `icon`, `icon-button`,
`separator`, `stack`; mais os tokens de paleta, os papéis semânticos e o tema claro/escuro.

O que a app precisa, por ordem de necessidade, e como se arranja enquanto a peça não existe:

| Prioridade | Peça | Na `line://ui` | Até existir | Ecrãs |
|---|---|---|---|---|
| P0 | botão, botão de ícone, grupo de botões | existe | | todos |
| P0 | badge de estado, alerta, separador, stack | existe | | overview, listagens, erros |
| P0 | filtros (chips) | existe | | tools, prompts, resources |
| P0 | campo de texto com label e erro | **falta** | `<input>` e `<label>` nativos com tokens | login, formulários |
| P0 | tabela com estado vazio | **falta** | `<table>` nativa com tokens | todas as listagens |
| P0 | navegação lateral, ligação | **falta** | `<nav>` e `<a>` nativos com tokens | casca |
| P0 | spinner / skeleton | parcial (loading do botão) | CSS da app | estados a carregar |
| P1 | diálogo (confirmação; "token mostrado uma vez") | **falta** | `<dialog>` nativo | apagar, criar user e MCP client |
| P1 | switch | parcial (botão toggle) | botão toggle | activar/desactivar |
| P1 | toast | parcial (alerta live) | alerta live | resultado de uma acção |
| P1 | select, textarea | **falta** | elementos nativos | registar servidor |
| P1 | tabs | **falta** | secções na mesma página | detalhe de servidor |
| P2 | combobox, menu, tooltip | **falta** | select nativo; botões por linha; `title` | grants, acções por linha |
| P2 | editor de pares chave-valor | composto, da app | | `env` e headers |

Regra: o que não existir na `line://ui` **não se substitui por outra biblioteca**. Usa-se o
elemento HTML nativo com os tokens `--line-*`, isolado num elemento `trunks-*`, para que a
troca pela peça da `line://ui` seja local quando ela existir.

Para a `line://ui`, a ordem que mais serve o Trunks é: campo de texto, tabela, diálogo,
select, tabs, switch, toast, menu, tooltip, combobox.

## 12. Entrega por partes

Com o que existe hoje, nenhuma parte fica bloqueada: as primitivas em falta têm um elemento
nativo que serve. O que muda é a qualidade do resultado, e por isso a ordem:

| Parte | Conteúdo | Com a `line://ui` de hoje |
|---|---|---|
| **6c-1** | esqueleto Astro, servir no Rust, geração de tipos, login, overview, CI `web` e fumo mínimo | pode arrancar; o campo do login é nativo |
| **6c-2** | catálogo só de leitura: servidores e detalhe, tools, prompts, resources, grupos, diagnóstico | pode arrancar; tabelas nativas, filtros com chips |
| **6c-3** | gestão em development: registar (incl. OAuth), activar/desactivar, apagar, grupos | possível, mas é onde mais se nota a falta de campo, diálogo e select; melhor depois de existirem |
| **6c-4** | gestão em enterprise: users, teams, MCP clients, grants, conta | idem, mais combobox e menu |

Cada parte é um PR com o seu fumo. A 6c-1 e a 6c-2 dão uma consola só de leitura utilizável;
o CLI cobre o resto enquanto as partes seguintes não chegam.

## 13. Decisões desta definição

| # | Decisão | Alternativa rejeitada | Porquê |
|---|---|---|---|
| W1 | Astro estático, sem SSR | SPA em Lit + Vite (definição anterior); Astro com adapter Node | um binário sem Node; páginas como documentos são mais simples de servir, testar e manter do que um router de cliente |
| W2 | Sem framework de UI; interactividade em custom elements | ilhas React/Preact/Svelte | a `line://ui` já é Web Components; um framework por cima duplicava o modelo de componentes |
| W3 | Elementos da app em Lit, prefixo `trunks-*` | scripts soltos por página | o Lit já é dependência; dá ciclo de vida e reactividade sem código de cola |
| W4 | Detalhe por query string | `/servers/{name}` com reescrita no Rust | rotas desconhecidas no build; zero casos especiais no servidor e no `astro dev` |
| W5 | Estáticos públicos, dados atrás da API | estáticos atrás de sessão (texto original da D15) | a casca não tem dados e o login tem de abrir sem sessão |
| W6 | Tipos TypeScript gerados dos tipos Rust, com teste de deriva | tipos escritos à mão; OpenAPI | uma só fonte para os contratos, sem um documento OpenAPI para manter |
| W7 | Sem `<ClientRouter />` na primeira versão | view transitions desde o início | menos ciclo de vida para gerir; pode entrar depois sem mudar a arquitectura |
| W8 | Entrega em quatro partes, as duas primeiras só de leitura | esperar pela `line://ui` completa | a consola de leitura é útil cedo e fixa a arquitectura |
| W9 | Peça em falta na `line://ui` → elemento HTML nativo com tokens, dentro de um `trunks-*` | outra biblioteca de componentes; esperar | não entra um segundo design system; a troca posterior fica contida num elemento |

## 14. Referência de design

O design da consola foi feito no Claude Design a partir desta definição (canvas "Trunks
Design", privado do autor): 35 artboards com os ecrãs da secção 5, as variantes em tema claro
e a 390 px, uma folha com os componentes novos e os seus estados, uma de padrões (estados das
regiões de dados, truncagem, copiar, polling) e uma de tokens. É a referência visual e de
comportamento da implementação.

O que vem do design para o repositório:

- `web/app/src/styles/console.css`: a folha de estilos do design, que só usa tokens
  `--line-*` (accent `teal`, gray `slate`). Os valores dos tokens vêm dos pacotes
  `@websublime/line-tokens`, `line-colors` e `line-themes`, não do design.
- Os ícones (`web/app/src/lib/icons.ts`) e a marca.
- Os textos e os estados de cada ecrã.

Onde a implementação se afasta do design, porque o design assumia outra coisa:

| No design | Na implementação | Porquê |
|---|---|---|
| navegação filtrada no servidor | filtrada no browser, a partir de `/auth/config` e do `whoami` | páginas estáticas (W1) |
| tabs como caminhos (`/servers/github/tools`) | query string (`?name=github&tab=tools`) | W4 |
| token errado recarrega a página | erro no próprio formulário, campo limpo e com foco | páginas estáticas |
| 403 e "not initialized" servidos pelo servidor | estados mostrados pela app a partir da resposta da API; página `/not-initialized/` | páginas estáticas |
| role `member` | `user` | é o nome na API |
| endpoint de grupo `/groups/<nome>/mcp` | o que a API devolve (`/v0/groups/<nome>/mcp`) | contrato |
| estilos inline | classes | CSP sem `unsafe-inline` |
| tema escuro por omissão | segue a preferência do sistema (`color-scheme: light dark`) | os dois temas estão desenhados e passam AA |
| criar user com escolha de role | sem o campo: a API cria sempre `user` | administradores só nascem do `init-server` e do SSO |
| registar com OAuth: "registado, autoriza depois" | o servidor só fica registado quando a autorização termina (10 min); sem "Do it later" | é o que o registry faz |
| conta: "via team platform" | o `whoami` passa a devolver os grants do user e das suas teams (6c-4) | o design precisa da origem de cada acesso |
| sem ecrãs para apagar ou desactivar um servidor, nem para criar, alterar ou apagar um tool group | desenhados na implementação com o vocabulário do design: interruptor e botão "Delete" no cabeçalho do detalhe, confirmação no `alertdialog` dos componentes, formulário de grupo com uma caixa por servidor | a definição (§12, 6c-3) pede-os e o design não os cobriu |
| session mode num botão com listbox | `<select>` nativo | W9: a `line://ui` ainda não tem select |
| registo OAuth sem cliente próprio | secção recolhível "Authorization with your own OAuth client" com o redirect URI a copiar | providers sem registo dinâmico de clientes |

O design mediu cinco pontos em que os tokens da `line://ui` não cumprem AA e contorna-os na
raiz da consola: `--line-accent-contrast` (branco sobre teal-9, 3,07:1), a cor do anel de foco
(2,30:1 no tema claro), a borda dos controlos (gray-7, 1,53:1), o botão danger sólido (3,91:1)
e o texto dos badges (passo 11 sobre passo 3). São candidatos a corrigir na biblioteca; os
dois primeiros estão em `console.css` como overrides.

Estado da `line://ui` no npm (2026-11-11): `@websublime/line-components` 0.2.4 publica
`line-button`, `line-stack` e `line-separator`. O Storybook mostra também alert, badge, chip,
icon, icon-button e button-group, ainda não publicados; até lá seguem a regra W9.

