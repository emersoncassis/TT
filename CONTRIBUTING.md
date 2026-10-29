# Contribuindo

1. **Clean-room:** não copie código, texto, ícones, nomes ou recursos de software proprietário. Descreva o comportamento pela documentação pública do protocolo/equipamento. Ao abrir um PR você confirma que seguiu [docs/CLEAN-ROOM.md](docs/CLEAN-ROOM.md).
2. `cargo test --workspace` deve passar.
3. Novos tipos de equipamento: adicione um JSON em `profiles/` e cite a documentação oficial dos comandos em `docs/PLATFORMS.md`.
