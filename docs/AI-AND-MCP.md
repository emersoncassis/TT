# Assistente de IA e servidor MCP (design — ainda não implementado)

Princípio: **a IA sugere, o técnico decide.** Nada é executado sozinho.

## 1. Assistente assistido no terminal

- O usuário cadastra a **própria chave de API** (provedor configurável). Em modo servidor ela fica no cofre do usuário (criptografada), nunca é enviada ao navegador depois de salva.
- Painel de chat ao lado do terminal. O navegador envia à IA, sob demanda, um **recorte do que está na tela** (buffer do xterm das últimas N linhas) + o perfil do equipamento (ex.: "MikroTik RouterOS 7").
- A IA responde com explicações, diagnósticos, boas práticas e **comandos sugeridos**. Cada comando vira um cartão com ações **Colar no terminal** e **Executar**; comandos marcados como sensíveis (reset, reboot, apagar, alterar firewall) exigem confirmação explícita.
- **Redação de segredos** antes de enviar: senhas, `enable secret`, chaves, tokens e blocos `-----BEGIN ... PRIVATE KEY-----` são mascarados no recorte. O usuário vê o que será enviado e pode desligar o envio da tela.
- Perfis do TT alimentam o prompt do sistema (dialeto da CLI, comandos de leitura seguros, avisos específicos da plataforma).
- Registro de auditoria (modo servidor): quem pediu, para qual host, o que foi sugerido/executado.

## 2. TT como servidor MCP

Para que clientes de IA (assistentes, IDEs) usem o TT como ferramenta:

- Transporte: stdio (`tt mcp`) e HTTP autenticado por token pessoal no modo servidor.
- Ferramentas propostas:
  - `list_hosts` — hosts do usuário (sem segredos);
  - `read_terminal(host, lines)` — recorte da sessão aberta (com redação de segredos);
  - `suggest_command(host, command, reason)` — **não executa**: cria uma sugestão pendente que o técnico aprova na interface;
  - `run_readonly(host, command)` — apenas comandos de uma lista de leitura segura do perfil (ex.: `show version`, `/system resource print`), com limite de tempo/saída;
  - `sftp_list(host, path)` — somente leitura.
- Operações que alteram o equipamento **sempre passam por aprovação humana** na interface. Escopo por token (hosts permitidos, somente leitura) e limite de taxa.
- Também publicado como **skill** com as boas práticas por plataforma (os perfis de `profiles/`).

## 3. Ordem de implementação

1. API de captura de tela + redação de segredos (testável sem IA).
2. Painel de chat com chave própria e cartões de comando.
3. `tt mcp` (stdio) com `list_hosts`, `read_terminal`, `suggest_command`.
4. Lista de comandos de leitura segura por perfil + `run_readonly`.
5. Transporte HTTP com tokens e auditoria.
