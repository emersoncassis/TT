# Especificação do TT

## Objetivo

Alternativa livre (MIT) a uma workstation SSH completa, com dois modos de execução do mesmo núcleo:

- **Local:** uso individual; o servidor embutido escuta apenas em `127.0.0.1` e entra sem senha por um token de lançamento.
- **Servidor web:** o painel é acessado pela rede (usuário e senha); **quem se conecta aos equipamentos é a máquina onde o TT roda**. O primeiro acesso cria o administrador; ele cria os demais usuários. Cada usuário enxerga somente os próprios hosts.

## Arquitetura

```
 navegador (xterm.js, UI sem build)  ──HTTP/WebSocket──▶  tt (axum)
                                                         ├─ tt-core: russh (SSH), russh-sftp, NETCONF, perfis, cofre
                                                         └─ SQLite (usuários, sessões de login, hosts, chaves confiadas)
                                                                    │ SSH / SFTP / NETCONF
                                                                    ▼
                                                     servidores Linux, roteadores, firewalls, switches
```

- `crates/tt-core`: biblioteca sem dependência de web. Reutilizável por um futuro app desktop e pelo servidor MCP.
- `crates/tt`: binário `tt` (CLI, API REST, WebSockets, UI embutida).
- `ui/`: HTML/CSS/JS puro; tokens de tema em `ui/theme.css`.
- `profiles/`: perfis de equipamento em JSON (comunidade).

## Matriz de funcionalidades

Categorias baseadas na descrição pública de workstations SSH tradicionais.

| Funcionalidade | Estado |
|---|---|
| Terminal SSH com abas | ✅ v0.1 |
| Autenticação por senha / chave privada / keyboard-interactive | ✅ v0.1 |
| Verificação de chave de host (TOFU + alerta de mudança) | ✅ v0.1 |
| Gerenciador de sessões (pastas, busca) | ✅ v0.1 |
| Navegador SFTP (listar/enviar/baixar/pasta/renomear/excluir) | ✅ v0.1 |
| Perfis de equipamento + comandos prontos | ✅ v0.1 |
| Console NETCONF | ✅ v0.1 (framing validado por testes; falta teste contra equipamento real) |
| Usuários, administrador, login com limite de tentativas | ✅ v0.1 |
| Modo local sem senha | ✅ v0.1 |
| Marca personalizável | ✅ v0.1 |
| Arrastar e soltar arquivos / upload em streaming | 🔜 |
| Túneis SSH local e dinâmico/SOCKS5 — RFC 4254 | ✅ v0.2 |
| Túnel remoto (`-R`) | 🔜 |
| Multi-execução (mesmo comando em vários hosts) | 🔜 |
| Macros e snippets do usuário | 🔜 |
| Salto por bastion (ProxyJump) | 🔜 |
| Geração/gestão de chaves SSH | 🔜 |
| TLS nativo, 2FA (TOTP), SSO | 🔜 |
| Gravação/auditoria de sessões (modo servidor) | 🔜 |
| Telnet e serial (equipamentos legados) | 🔜 |
| Encaminhamento X11 | 🔜 |
| RDP / VNC | 🔜 (via gateways abertos, ex.: Guacamole) |
| App desktop empacotado (Tauri) | 🔜 |
| **Assistente de IA assistido** (chave própria, só sugere) | ✅ v0.2 [AI-AND-MCP.md](AI-AND-MCP.md) |
| **Servidor MCP** (HTTP + stdio, tokens, auditoria) | ✅ v0.2 |
| IA com *streaming* | 🔜 |
| Tradução da interface (i18n) | 🔜 (hoje pt-BR) |

## Decisões

- **Rust + russh:** binário único, sem runtime; SSH em Rust seguro.
- **Segredos:** XChaCha20-Poly1305 com chave mestra em arquivo. Ainda não há senha mestra do usuário; fica no roadmap para o modo local.
- **SFTP exige chave já confiada:** evita que uma operação de arquivos aceite uma chave desconhecida sem o usuário ver.
- **Design:** segue o design system IC TEAM (escuro primeiro, ouro, Barlow Condensed + Inter). Todo o visual está nas variáveis de `ui/theme.css`. As fontes não são embutidas (sem chamadas externas); sem elas a interface usa Arial Narrow/fonte do sistema.
