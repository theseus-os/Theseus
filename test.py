my_bytes = []
for value in range(2):
	my_bytes.append(value)
newFileByteArray = bytearray(my_bytes)
target = open("random_data2.img","w")
target.write(newFileByteArray)