select '- <@' || u.discord_user_id || '> - ' || count(distinct r.id) || ' requests completed'
  from request r
       inner join task t on t.request = r.id
       inner join "user" u on t.assigned_to = u.id
 where r.created_at > '2024-06-17 00:00:00Z'
   and t.completed_at is not null
 group by u.discord_user_id
 order by count(distinct r.id) desc;
