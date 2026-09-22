# Project Chronos Engine 1.0 alpha

Première préversion publique réunissant le firmware Chronos et l'éditeur de
bibliothèques pour PC Engine Mini.

## Téléchargements

| Fichier | Contenu |
|---|---|
| `chronos-p6.zip` | Partition P6 : noyau personnalisé Linux 3.4.113. |
| `chronos-p7.zip` | Partition P7 : système Chronos, démarrage USB et accès SSH/SFTP. |
| `PCE-Game-Editor-1.0.0-alpha-windows-x64.zip` | Éditeur Windows x64 : exécutable direct et installateur. |
| `PCE-Game-Editor-1.0.0-alpha-macos-arm64.zip` | Application `PCE Game Editor.app` pour Mac Apple Silicon. |
| `SHA256SUMS.txt` | Empreintes de vérification des téléchargements. |
| `manifest.json` | Provenance des images, commits et contrôles des applications. |

Les images P6/P7 sont les copies inchangées d'un dump frais de la console
fonctionnelle, réalisé le 21 septembre 2026. La configuration du noyau et les
instructions détaillées sont incluses dans les archives firmware.

## Installation sur la console

1. Sauvegarder les partitions de la console avec PCE Mini Recovery.
2. Passer la console en recovery, puis restaurer `chronos-p6.zip` sur **P6**
   et `chronos-p7.zip` sur **P7**, en laissant terminer les vérifications.
3. Conserver **P8 et P9** : ces partitions ne sont pas fournies par cette release.
4. Préparer une clé FAT32 avec l'éditeur. Placer les dossiers `game/` et
   `library/` publiés à la racine, puis redémarrer la console avec la clé.

Le firmware a été validé sur PC Engine Mini JP / Allwinner A33. Les autres
variantes de la console ne sont pas encore validées. SSH et SFTP sont activés
sur **169.254.13.37:22**, utilisateur **root**, mot de passe vide. Les clés SSH
propres à chaque console sont générées dans P8 ; les images ne contiennent pas
de clé SSH privée.

## Éditeur

- Créer une bibliothèque vide ou avec les jeux d'origine depuis un dump complet,
  des partitions séparées ou un dossier P9 extrait.
- Organiser les jeux en lineups JP/US et dossiers, modifier les jaquettes,
  métadonnées et styles, puis publier la clé USB Chronos.
- Importer les HuCards `.pce`, `.sgx` et `.pce.m`. Les `.pce.m` sont copiés sans
  décompression, avec leur nom et leur contenu conservés.
- Importer un `.pcd` directement ou convertir un `.cue` avec ses pistes en
  `.pcd`, avec progression et configuration des BIOS dans les paramètres.
- Consulter les sauvegardes et la SRAM associées aux jeux.
- Choisir **No titlebar**, y compris côté US, comme pour Neutopia II JP.

**Windows :** décompresser le ZIP et lancer `pce-game-editor.exe`, ou utiliser
l'installateur fourni. L'exécutable direct nécessite Microsoft WebView2 ;
l'installateur le télécharge si nécessaire.

**Mac Apple Silicon :** décompresser le ZIP et copier `PCE Game Editor.app`
dans Applications. Cette archive contient uniquement l'architecture ARM64.

Les éditeurs ne contiennent aucun jeu ni BIOS. La création d'une bibliothèque
utilise le dump d'origine de l'utilisateur. Les applications ne disposent pas
encore d'un certificat d'éditeur Windows ou d'une notarisation Apple.

Il s'agit d'une **version alpha**. Conserver les sauvegardes de la console et
de la bibliothèque avant de tester. La version Linux n'est pas incluse.
