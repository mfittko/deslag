### Changed

- The `lines-v2` encoder no longer stops templating below back-to-back or periodic outlier lines; it writes the best-paying group prefix and stops a group before a run only when the run directive saves more than the run's copies would as group rows. The `line-templates` transform version is now 2 (#59)
