# Política clean-room

O TT reimplementa **funcionalidades** comuns a workstations SSH (abas de terminal, gerenciador de sessões, navegador SFTP, túneis, etc.). Funcionalidades e ideias não são protegidas por direito autoral; a implementação, o código, a arte e os textos são.

## Regras

1. **Fonte permitida:** documentação pública de protocolos (RFCs), documentação oficial de equipamentos, a descrição pública de funcionalidades (páginas de produto/manuais) e o conhecimento de uso dos contribuidores.
2. **Proibido:** descompilar, desmontar, inspecionar o conteúdo de instaladores/binários/plugins de software proprietário, copiar código, recursos gráficos, ícones, nomes de menus, mensagens, strings de interface, documentação ou estrutura de arquivos de configuração.
3. **Nomes e marca:** o TT não usa nome, logotipo ou identidade visual de terceiros. A interface é desenhada do zero.
4. **Arquivos de terceiros fora do repositório:** instaladores e binários proprietários (ex.: `MobaXterm*.exe`, `*.msi`, `*.plugin`) nunca devem ser commitados. O `.gitignore` já os bloqueia; não os remova.
5. **Contribuições:** ao abrir um PR, o autor declara que seguiu estas regras.

## Como especificar uma funcionalidade

Descreva *o que o usuário consegue fazer* (ex.: "abrir um túnel local para uma porta remota") e implemente a partir do protocolo (RFC 4254 para port forwarding), não a partir de outro produto.
