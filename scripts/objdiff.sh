objdump -d -C $1 > /tmp/objdiff1 
objdump -d -C $2 > /tmp/objdiff2
objdump -d -C $3 > /tmp/objdiff3
objdump -d -C $4 > /tmp/objdiff4
vimdiff /tmp/objdiff1 /tmp/objdiff2 /tmp/objdiff3 /tmp/objdiff4
