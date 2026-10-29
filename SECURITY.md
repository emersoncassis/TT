# Segurança

Encontrou uma vulnerabilidade? Abra um *security advisory* privado no GitHub (aba **Security** do repositório) em vez de uma issue pública.

Limites conhecidos da v0.1: sem TLS nativo (use proxy HTTPS); upload/download de arquivos passa inteiro pela memória do servidor (limite de 512 MB por arquivo); certificados de host SSH (OpenSSH CA) ainda não são suportados; sem 2FA.

IA/MCP: a redação de segredos é por padrões (melhor esforço). Endereço de API de IA personalizado só para administradores. Tokens MCP dão acesso aos hosts do usuário (use `read` quando bastar e revogue os que não usa).
