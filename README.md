# TT — Terminal Tools

Estação de trabalho SSH **livre e aberta** para quem administra servidores e equipamentos de rede.
Um único programa, dois modos:

| Modo | Comando | Para quê |
|------|---------|----------|
| **Local** | `tt local` | Uso individual na sua máquina. Abre o navegador já logado (sem senha), escuta só em `127.0.0.1`. |
| **Servidor web** | `tt serve --bind 0.0.0.0:8443` | A sua máquina (ou um servidor) é quem fala SSH com os equipamentos; o painel é acessado pela rede, com **usuário e senha**. O primeiro acesso cria o **administrador**, que cria os demais usuários. |

> Projeto *clean-room*: reimplementa **funcionalidades** de uma workstation SSH tradicional, sem copiar código, arte, nomes ou textos de nenhum software. Veja [docs/CLEAN-ROOM.md](docs/CLEAN-ROOM.md).

## O que já funciona (v0.1)

- Terminal SSH no navegador (xterm.js), abas, redimensionamento, autenticação por **senha, chave privada e keyboard-interactive**.
- **Verificação de chave do host** (confiar na primeira vez; alerta forte se a chave mudar).
- Gerenciador de sessões com pastas, busca e **segredos criptografados em repouso** (XChaCha20-Poly1305).
- **Gerenciador de arquivos SFTP** (listar, enviar, baixar, criar pasta, renomear, excluir).
- **Perfis de equipamento** com facilitadores: Debian/Ubuntu, Raspberry Pi, OpenWrt, pfSense, OPNsense, MikroTik, Cisco IOS/IOS-XE, Juniper Junos, Huawei VRP, NETCONF e genérico. Cada perfil traz comandos prontos, ajustes automáticos de login e avisos — veja [docs/PLATFORMS.md](docs/PLATFORMS.md).
- **Console NETCONF** (RFC 6241/6242, framing 1.0 e 1.1) com modelos de operações.
- Usuários e administrador, senhas com Argon2id, limite de tentativas de login, proteção contra requisições cross-site.
- Marca personalizável (nome, cor, logo) por arquivo — sem recompilar.

Roadmap (túneis, X11, RDP/VNC, multi-execução, macros, **assistente de IA** e **servidor MCP**): [docs/SPEC.md](docs/SPEC.md) e [docs/AI-AND-MCP.md](docs/AI-AND-MCP.md).

## Rodando

```bash
cargo run --release -- local                       # modo local
cargo run --release -- --data ./tt-data serve      # modo servidor em 127.0.0.1:8443
```

Dados (banco, chave mestra, perfis próprios, marca) ficam em `--data` (ou `TT_DATA`). **Faça backup de `master.key` junto com `tt.db`**: sem a chave mestra os segredos salvos não são recuperáveis.

### Expondo na rede com segurança

O TT fala HTTP simples. Para acesso pela rede coloque-o atrás de um proxy HTTPS (Caddy, nginx, Traefik) e use `--secure-cookies`. O TT avisa quando escuta fora do loopback sem isso.

### Personalizando a marca

Crie `tt-data/branding.json`:

```json
{ "name": "Minha Marca", "tagline": "Ferramentas de campo", "accent": "#00a3ff", "logo": true }
```

e coloque `tt-data/logo.svg` (ou `.png`, `.webp`, `.jpg`). Para mudar todo o tema edite as variáveis em `ui/theme.css`.

### Adicionando um tipo de equipamento

Crie um JSON em `tt-data/profiles/` (ou envie um PR em `profiles/`) no formato dos arquivos existentes. Perfis próprios com o mesmo `id` substituem os embutidos.

## Segurança em resumo

Senhas de usuários: Argon2id. Segredos de hosts: XChaCha20-Poly1305 com chave mestra em `master.key` (0600). Sessão por cookie `HttpOnly` + `SameSite=Strict`, verificação de `Origin` em requisições que alteram estado e em WebSockets, bloqueio após 8 tentativas de login por IP. O SFTP só conecta a hosts cuja chave já foi confiada em um terminal. Relatos de vulnerabilidade: veja [SECURITY.md](SECURITY.md).

## Licença

MIT — use, modifique, troque a logo, distribua. Veja [LICENSE](LICENSE).
