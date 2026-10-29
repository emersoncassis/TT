# Assistente de IA e servidor MCP

Princípio: **a IA sugere, o técnico decide.** Nada é executado sozinho.

## 1. Assistente no terminal (implementado)

Botão **Assistente** ao lado de cada terminal.

- **Chave própria.** Em *IA e MCP* o usuário escolhe o provedor — **Anthropic (Claude), Google (Gemini), OpenRouter** ou OpenAI/qualquer servidor compatível —, o modelo e cola a chave de API. A chave é criptografada no servidor e nunca volta ao navegador. Endereço de API personalizado (ex.: um LLM local) só pode ser definido por administradores, porque faz o servidor abrir conexões para a URL informada.
- **Contexto.** A cada pergunta o navegador envia o que está na tela (as últimas linhas do terminal). O servidor **mascara segredos antes de enviar ao modelo** (chaves privadas, `secret`/`password`, `snmp community`, PSK/WPA, tokens) e inclui o tipo de equipamento e as notas do perfil no prompt do sistema. O usuário pode desligar o envio da tela.
- **Só sugere.** As respostas trazem os comandos em cartões com **Colar** e **Executar**. Comandos que estão na lista de leitura do perfil têm execução direta; os demais mostram aviso e pedem confirmação.
- **Injeção de prompt.** A tela é enviada dentro de `<terminal_screen>` e o prompt manda tratá-la como dado não confiável.
- **Auditoria.** Cada pergunta gera um registro (sem o conteúdo).

### Escolhendo provedor e modelo (custo-benefício)

| Provedor | Quando usar | Observações |
|---|---|---|
| **Gemini (Google)** | Uso diário barato: os modelos *Flash* costumam dar o melhor custo-benefício, e há camada gratuita com limite de uso. | Na camada gratuita o Google pode usar o conteúdo enviado para melhorar seus produtos: para equipamento de cliente, prefira a camada paga. Modelo padrão: `gemini-3.8-flash`. |
| **OpenRouter** | Uma chave para vários modelos, e **modelos gratuitos** (ids terminados em `:free`). O botão *Listar modelos gratuitos* consulta a lista pública do OpenRouter na hora, porque esses ids mudam com frequência. | Modelos gratuitos têm limite de uso e o provedor pode registrar o que você envia. Bom para estudo e laboratório. |
| **Anthropic (Claude)** | Problemas difíceis (roteamento, firewall, configuração conflitante) em que a qualidade do raciocínio pesa mais que o preço. | Modelo padrão: `claude-sonnet-5-5`; um modelo menor sai mais barato no dia a dia. |
| **OpenAI ou compatível** | Servidor próprio ou modelo local, que mantém tudo dentro da sua rede. | Endereço da API só para administradores. |

Estratégia simples: modelo pequeno ou gratuito para explicar saídas e sugerir comandos de rotina; modelo maior só quando travar. Em ambiente de cliente, use plano pago/sem treino e deixe *enviar a tela* ligado só quando precisar: a redação de segredos é por padrões e **o restante da tela vai ao provedor**.

## 2. TT como servidor MCP (implementado)

Para que outro assistente de IA use o TT como ferramenta.

- **Transportes:** HTTP `POST /mcp` com `Authorization: Bearer <token>`, e `tt mcp --url http://servidor:8443 --token tt_...` para clientes que abrem um processo (ponte stdio).
- **Tokens pessoais** (criados em *IA e MCP*, mostrados uma vez, guardados só como hash):
  - `read`: ler e sugerir.
  - `exec`: também `run_readonly`.
- **Ferramentas:**

| Ferramenta | O que faz | Observação |
|---|---|---|
| `list_hosts` | Hosts do usuário | sem segredos |
| `list_sessions` | Terminais abertos | |
| `get_device_profile` | Notas, comandos e prefixos de leitura do tipo de equipamento | |
| `read_terminal` | Últimas linhas de um terminal aberto | sem escapes ANSI e com segredos mascarados |
| `suggest_command` | Propõe um comando | **não executa**: aparece no assistente e o técnico aprova |
| `run_readonly` | Roda UM comando de leitura | só o que o perfil permite (`readonly` no JSON); sem `; & | > < $ ( )` e sem crases, exceto `|` nos perfis de CLI de rede; exige token `exec`; saída limitada e mascarada |

- **Requisitos de segurança:** o host precisa ter a chave já confiada num terminal; tudo que mexe no equipamento passa por aprovação humana; todas as chamadas vão para o log de auditoria (`GET /api/audit`, só administradores).
- **Lista de leitura por perfil:** campo `readonly` em `profiles/*.json`. Entradas terminadas em `$` exigem correspondência exata. Cisco/Junos/Huawei liberam `show`/`display` (com pipe); MikroTik libera comandos `print` conhecidos; Linux e firewalls liberam uma lista curta. Contribuições para ampliar a lista com segurança são bem-vindas.

## 3. Skill

`skill/tt-network-assistant/SKILL.md` ensina um assistente a trabalhar com o TT: sugerir em vez de executar, ler antes de escrever, avisar riscos e as boas práticas de cada plataforma.

## Limites conhecidos

- Respostas do modelo chegam inteiras (sem *streaming*).
- A redação de segredos é por padrões (melhor esforço): não garante que nada vaze. Revise o que envia em ambientes sensíveis ou desligue o envio da tela.
- `suggest_command` e `read_terminal` exigem um terminal aberto no navegador; `run_readonly` abre uma conexão própria.
- `run_readonly` em equipamentos que não aceitam comandos fora de shell interativo (alguns Cisco antigos) pode falhar.
- Funcionamento contra modelos reais foi validado apenas com um servidor simulado dos dois formatos de API; teste com a sua chave e relate problemas.
