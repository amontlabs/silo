---
"silo-ui": patch
---

Review requests no longer stay open after their fork is deleted. Deleting a computer closes the open requests that name it as the fork or the target, `silo requests` shows them as "closed: fork deleted" or "closed: target deleted", and the list closes any left over once it can see every computer. Forking from a step now closes the fork popover instead of reporting that the new computer's name is taken.
