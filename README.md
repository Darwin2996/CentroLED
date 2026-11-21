# Centro LED

App nativa de Windows (Rust + Win32, ~750 KB) para controlar las luces y ver la batería de tus periféricos. En segundo plano usa unos pocos MB de RAM y prácticamente 0 % de CPU.

## Dispositivos

| Dispositivo | Batería | Luces |
|---|---|---|
| Teclado Tempest Diablo (0C45:8006) | — (con cable) | Teclas: 19 efectos o color libre tecla a tecla. Borde: 6 efectos propios con su color. Brillo, velocidad, arcoíris. Se guarda en el teclado |
| Mando DualSense / DualSense Edge (USB y Bluetooth) | Sí | Barra de luz: estático, respiración, **ciclo suave** entre 2–8 colores, arcoíris o apagada. LEDs de jugador (5, con brillo) y LED del micrófono (apagado/encendido/parpadeo) |
| Mando DualShock 4 | Sí | Barra de luz (mismos efectos) |
| Logitech HID++ (G733 y otros cascos/ratones/teclados) | Sí | Cada zona de luz por separado (p. ej. las dos luces del G733) con los efectos que admita el dispositivo: estático, respiración, pulso, ciclo de color, ondas… |
| Ratones VXE / ATK (receptor 2.4 GHz) | Sí, si el ratón está despierto (si duerme se muestra el último valor) | — |
| Mandos Xbox (One, Series X\|S, Elite, 360 y compatibles) | Sí (exacta por Bluetooth, aproximada por adaptador/USB); también la de los cascos Xbox conectados al mando | — |
| Mando Pro de Switch, Joy-Con y mando SNES | Sí (aproximada, por Bluetooth) | LEDs de jugador y luz del botón Home |
| Cascos SteelSeries Arctis 1, 7, 7+, 9, Nova 5, Nova 7 y Nova Pro Wireless | Sí | — |
| Cascos HyperX Cloud Alpha Wireless, Cloud Flight y Cloud II Wireless | Sí | — |
| Cascos Corsair Void (y variantes) | Sí | — |
| Cualquier dispositivo Bluetooth que informe batería a Windows (cascos, ratones, teclados, mandos, relojes…) | Sí | — |
| Cualquier dispositivo con luces **Dynamic Lighting** de Windows 11 (estándar HID LampArray: Razer, Logitech, ASUS, Corsair, Microsoft…) | — | Color fijo o apagado, sin programas extra |
| Ratones, teclados y cascos **Razer** | Sí (protocolo Razer) | Con Dynamic Lighting u OpenRGB |
| Cualquier dispositivo que publique su batería con el estándar HID (*Battery Strength*) | Sí | — |
| **Todo lo que soporta [OpenRGB](https://openrgb.org)**: placas base, RAM, gráficas, ventiladores, tiras LED, Razer, Corsair… | — | Color fijo o apagado (OpenRGB abierto con su *SDK Server* activado) |

Las luces de mandos y cascos se pierden al desconectarlos; la app las vuelve a aplicar sola (o reanuda el ciclo) cuando se reconectan.

## Uso

- Ejecuta `CentroLED.exe`. Al cerrar la ventana sigue en segundo plano en la bandeja del sistema. Clic derecho en el icono → Salir.
- **Teclado**: pestaña **Teclas** para el efecto o para pintar teclas (clic o arrastrar; clic derecho copia el color de una tecla); pestaña **Borde** para el efecto y color de la tira de luz. La vista previa se anima con el efecto elegido (es aproximada). Pulsa **Aplicar** para guardarlo en el teclado.
- **Mando**: elige "Ciclo suave", añade colores desde la paleta (clic en un color de la lista para quitarlo) y ajusta brillo y velocidad.
- **Inicio**: un resumen con una tarjeta por dispositivo (batería, estado y color de sus luces). **Ajustes**: todas las opciones en una página.
- **Batería en todas las pantallas**: Windows sólo pone iconos junto al reloj en la pantalla principal, así que en las demás Centro LED coloca un pequeño widget sobre la barra de tareas, junto a su reloj. Clic = abrir Centro LED, clic derecho = menú, arrastrar = moverlo. Se oculta solo con juegos o vídeos a pantalla completa en esa pantalla.
- **Batería en la barra de tareas**: cada dispositivo con batería tiene su icono junto al reloj: el dibujo del tipo de dispositivo (mando, cascos, ratón, teclado…) sobre una pila con el porcentaje (verde/ámbar/rojo, contorno amarillo al cargar, gris si es el último valor conocido). Se adapta al tema claro u oscuro de la barra. Se activa o desactiva en la barra lateral. Windows 11 coloca los iconos nuevos tras la flecha **^**: la app los marca para mostrarse y Windows lo aplica al reiniciar la sesión; o arrástralos una vez de la flecha a la barra.
- **Aviso de batería baja**: notificación de Windows al bajar del 15 % sin cargar (requiere tener las notificaciones de Windows activadas).
- **Perfiles**: en *Inicio* hay botones con tus perfiles (de ejemplo: *Juego*, *Noche* con poco brillo y *Apagado*, que no toca el teclado). Clic = aplicar, clic derecho = guardar ahí las luces actuales, cambiar nombre o borrar; **+ Guardar** crea uno nuevo con las luces de ahora. También desde el icono de la bandeja (*Perfiles*). Los dispositivos desconectados reciben el perfil al conectarse. Cada dispositivo tiene además sus propios perfiles en el botón **Perfiles ▾** de su página.
- **Todas las luces**: en *Inicio*, un color para todos los dispositivos a la vez y **Apagar todo / Encender todo** (vuelven exactamente como estaban).
- **Apagado automático** (*Ajustes*, todo desactivado de serie): apaga las luces al bloquear el equipo, al apagarse la pantalla, al suspender o al apagar el equipo, y las devuelve solas al volver, sin cambiar tu configuración. El teclado nunca se apaga solo; puedes elegir qué otros dispositivos participan.
- **Atajos de teclado** (*Ajustes*): graba la combinación que quieras para apagar/encender todas las luces o para cada perfil. Funcionan con la app en segundo plano y con juegos abiertos. Si otro programa ya usa la combinación, se avisa en rojo.
- **Historial de batería**: cada dispositivo con batería guarda 30 días de lecturas (`bateria\*.csv` junto a la configuración). En su página se ve la gráfica de 24 h o 7 días y el tiempo que le queda (o lo que tarda en cargarse) al ritmo de uso actual; la estimación sale también en las tarjetas de *Inicio*.
- **Diagnóstico** (*Ajustes*): guarda un `.txt` con la versión de Windows, las pantallas, los dispositivos reconocidos y todas las colecciones HID, para investigar un dispositivo que no va. Se guarda donde elijas, no se envía a ningún sitio y no incluye números de serie ni tu nombre de usuario (los nombres de dispositivos Bluetooth son los que les hayas puesto: revísalo antes de compartirlo).
- "Iniciar con Windows" la arranca minimizada en la bandeja (si mueves el `.exe`, la entrada se corrige sola al abrirlo). **Inicio rápido · prioridad alta** la arranca con una tarea programada nada más iniciar sesión, sin la espera que Windows pone a los programas de inicio y con prioridad alta mientras arranca (luego vuelve a la normal). No necesita permisos de administrador.
- **Apariencia** (*Ajustes*): seis temas (*Oscuro*, *Negro* para pantallas OLED, *Grafito*, *Océano*, *Claro* y *Sistema*, que sigue el modo claro u oscuro de Windows), ocho colores de acento (incluido el de Windows) y esquinas redondeadas o rectas. La barra de título y los menús cambian con el tema. La vista previa del teclado se queda oscura con cualquier tema.
- La ventana se adapta a la escala de cada pantalla (se ve nítida también en una secundaria con otra escala).
- La batería se actualiza cada minuto y al conectar/desconectar dispositivos.
- Configuración guardada en `%APPDATA%\CentroLED\config.txt`; si algo falla, el motivo queda en `%APPDATA%\CentroLED\error.txt`.

Línea de comandos (para scripts):

```
CentroLED.exe --probe salida.txt
CentroLED.exe --set <clave> <modo|off> <rrggbb> <brillo 0-100> <velocidad 0-100> salida.txt
CentroLED.exe --fast-start on|off salida.txt
CentroLED.exe --verify <archivo> <nombre> SHA256SUMS.txt SHA256SUMS.txt.sig salida.txt
```

## OpenRGB

Para controlar las luces de componentes del PC (placa base, RAM, gráfica…) y de cientos de periféricos más, instala [OpenRGB](https://openrgb.org), abre la pestaña **SDK Server** y pulsa **Start Server** (puedes activar que arranque minimizado con el servidor). Centro LED se conecta a él en `localhost:6742` y muestra sus dispositivos en la lista.

## Límites del teclado

El firmware del Diablo sólo ejecuta un efecto a la vez en las teclas (y otro en el borde) y sus efectos multicolor usan una lista de colores fija: no admite ciclos con colores elegidos ni segundo color. Cada cambio se graba en la memoria del teclado (~1 s), así que no se puede animar desde el PC.

## Notas

- Si usas DS4Windows con HidHide, añade `CentroLED.exe` a la lista de aplicaciones permitidas de HidHide para que vea el mando.
- Logitech G HUB puede sobrescribir las luces de los G733 si está abierto.
- El protocolo del teclado se obtuvo capturando el software oficial de Tempest.

## Descarga y actualizaciones

En la [última release](https://github.com/Darwin2996/CentroLED/releases/latest) hay tres archivos:

| Archivo | Para qué |
|---|---|
| `CentroLED-Setup.exe` | Instalador ligero: descarga la última versión, la instala para tu usuario (sin permisos de administrador) en `%LOCALAPPDATA%\Programs\CentroLED`, crea accesos directos y aparece en *Aplicaciones instaladas* para desinstalarla |
| `CentroLED-Portable.exe` | Versión portable: guarda la configuración en una carpeta `CentroLED-datos` junto al ejecutable (ideal para un USB). También funciona si dejas un `portable.txt` junto a cualquier `CentroLED.exe` |
| `CentroLED.exe` | El programa tal cual, sin instalar (configuración en `%APPDATA%\CentroLED`) |

La app comprueba sola si hay una versión nueva (al arrancar y cada 6 horas). Si la hay, bajo el título aparece el botón **Actualizar a vX**: descarga la versión nueva, sustituye el ejecutable y se reinicia. También puedes pulsar la línea de versión para comprobarlo en el momento.

**Actualizaciones firmadas** (desde la 1.5.0): cada release incluye `SHA256SUMS.txt` (la suma SHA-256 de cada archivo) y `SHA256SUMS.txt.sig` (su firma ECDSA P-256). La app y el instalador sólo instalan un ejecutable si la firma es válida con la clave pública que llevan dentro y la suma coincide; si falta algo o no cuadra, no instalan nada. La clave privada sólo está en el PC del autor (no en el repositorio), así que nadie más puede publicar una versión que se instale sola. Es una clave propia y gratuita, sin certificados de pago (por eso Windows SmartScreen puede seguir avisando la primera vez que abres el `.exe` descargado).

Para publicar una versión nueva: sube `version` en `Cargo.toml`, compila, copia `CentroLED.exe` también como `CentroLED-Portable.exe`, firma la carpeta con `powershell -ExecutionPolicy Bypass -File tools\firmar-release.ps1 <carpeta>` y crea una release con la etiqueta `vX.Y.Z` adjuntando los tres `.exe`, `SHA256SUMS.txt` y `SHA256SUMS.txt.sig`.

## Compilar

```
cargo build --release
```

## Créditos

Los protocolos de batería de cascos de otras marcas se basan en [HeadsetControl](https://github.com/Sapd/HeadsetControl) y los de los mandos de Switch en el controlador *hid-nintendo* de Linux.
