# Facilitadores por plataforma

Cada plataforma é um perfil JSON em `profiles/`. O perfil define porta padrão, se precisa de PTY, suporte a SFTP, ajuste do usuário de login, comandos executados ao entrar, passos automáticos pós-login e comandos prontos agrupados.

| Perfil | Ajustes automáticos | Observações |
|---|---|---|
| **Debian / Ubuntu** | — | Comandos de sistema, apt, systemd/journalctl, rede, UFW/nftables, Docker. |
| **Raspberry Pi** | — | `vcgencmd` (temperatura, throttling), `pinctrl`, `rpi-eeprom-update`, `raspi-config`. |
| **OpenWrt** | — | `uci`, `logread`, `ubus`, `sysupgrade -b`. O Dropbear não traz servidor SFTP por padrão: instale `openssh-sftp-server` para o gerenciador de arquivos. Releases novas usam `apk` no lugar do `opkg`. |
| **pfSense** | Escolhe a opção **8 (Shell)** do menu do console automaticamente | Menu: 8 Shell, 12 PHP shell + pfSense tools. Backup: `/conf/config.xml` via SFTP. |
| **OPNsense** | Escolhe a opção 8 (Shell) | `opnsense-version`, `configctl`. |
| **MikroTik RouterOS** | Acrescenta `+ct` ao usuário (sem cores, sem detecção de terminal) | `/export compact`, `/system backup`, `/ip ssh print` para conferir a fingerprint. Com chave pública cadastrada o RouterOS recusa senha por padrão. |
| **Cisco IOS / IOS-XE** | `terminal length 0`, `terminal width 0`; entra no modo `enable` se houver senha salva | Opção "algoritmos legados" para IOS antigo (DH group1/14-SHA1, AES-CBC, HMAC-SHA1). |
| **Juniper Junos** | `set cli screen-length 0`, `screen-width 0` | `show | compare`, `display set`. |
| **Huawei VRP** | `screen-length 0 temporary` | NETCONF/SNETCONF; roteadores domésticos HarmonyOS normalmente não expõem SSH. |
| **NETCONF** | Abre o subsistema `netconf` (porta 830) em vez de um shell | Framing 1.0 (`]]>]]>`) e 1.1 (chunked), conforme o `<hello>`. |

## Fontes

- MikroTik, opções de login `+c`, `+t`, `+<n>w`, `+<n>h`: [manual de Console RouterOS](https://manual.mikrotik.com/docs/7.25/management-tools/console); servidor SSH: [manual SSH RouterOS](https://manual.mikrotik.com/docs/7.25/management-tools/ssh).
- pfSense, menu do console: [Netgate — Console Menu](https://docs.netgate.com/pfsense/en/latest/config/console-menu.html); shell PHP: [Netgate — PHP Shell](https://docs.netgate.com/pfsense/en/latest/development/php-shell.html).
- NETCONF sobre SSH: [RFC 6242](https://rfc-editor.org/rfc/rfc6242) (porta 830, framing).
- OpenWrt/Dropbear sem `sftp-server`: [discussão no fórum OpenWrt](https://forum.openwrt.org/t/ash-usr-libexec-sftp-server-not-found-when-using-scp/125772).

## A validar com equipamento real

Comandos de Cisco, Junos, Huawei VRP, OPNsense e Raspberry Pi foram escritos a partir da documentação/conhecimento público e **ainda não foram testados em hardware**. Os fluxos de login automáticos (pfSense/OPNsense menu 8, Cisco enable) foram implementados mas só o mecanismo genérico foi testado contra um sshd. Issues e PRs com correções são bem-vindos.
