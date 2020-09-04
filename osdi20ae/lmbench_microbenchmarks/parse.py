static_mean = [0,0,0,0,0]
static_std_dev = [0,0,0,0,0]

loadable_mean = [0,0,0,0,0]
loadable_std_dev =[0,0,0,0,0]

for line in open('./loadable/results.txt','r'):
    if 'BM:' in line:
        tokens = line.split()
        if 'null' in tokens:
            loadable_mean[0] = float(tokens[4])
            loadable_std_dev[0]= float(tokens[5])
        if 'ctx' in tokens:
            loadable_mean[1] = float(tokens[4])
            loadable_std_dev[1]= float(tokens[5])
        if 'spawn' in tokens:
            loadable_mean[2] = float(tokens[4])
            loadable_std_dev[2]= float(tokens[5])
        if 'mem_map' in tokens:
            loadable_mean[3] = float(tokens[4])
            loadable_std_dev[3]= float(tokens[5])
        if 'ipc' in tokens:
            loadable_mean[4] = float(tokens[4])
            loadable_std_dev[4]= float(tokens[5])

for line in open('./static/results.txt','r'):
    if 'BM:' in line:
        tokens = line.split()
        if 'null' in tokens:
            static_mean[0] = float(tokens[4])
            static_std_dev[0]= float(tokens[5])
        if 'ctx' in tokens:
            static_mean[1] = float(tokens[4])
            static_std_dev[1]= float(tokens[5])
        if 'spawn' in tokens:
            static_mean[2] = float(tokens[4])
            static_std_dev[2]= float(tokens[5])
        if 'mem_map' in tokens:
            static_mean[3] = float(tokens[4])
            static_std_dev[3]= float(tokens[5])
        if 'ipc' in tokens:
            static_mean[4] = float(tokens[4])
            static_std_dev[4]= float(tokens[5])


print(loadable_mean)
print(loadable_std_dev)

print(static_mean)
print(static_std_dev)