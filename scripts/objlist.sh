echo "" > /tmp/objfiles
for objfile in "$@"
do
	echo "######################################################################" >> /tmp/objfiles
	echo "$(basename $objfile)" >> /tmp/objfiles
	echo "######################################################################" >> /tmp/objfiles
	objdump -d -C $objfile >> /tmp/objfiles
	echo -e "\n\n" >> /tmp/objfiles
done

vim /tmp/objfiles

